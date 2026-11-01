//! RFC-ACDP-0018 key material: the `/changes` cursor and the at-rest seal
//! for subscription secrets.
//!
//! One 32-byte master key (`[changes] key_b64 | key_path`) is expanded with
//! HKDF-SHA-256 into three independent subkeys, one per purpose, so the
//! cursor AEAD, the cursor nonce PRF and the secret seal never share a key.
//!
//! **Cursor.** `base64url_nopad(0x02 ‖ key_id ‖ kid4 ‖ nonce12 ‖ GCM(plain) ‖
//! tag16)` with `plain = pos_be8 ‖ anchor_seq_be8 ‖ anchor_fp8 ‖ epoch_be4` —
//! 62 bytes, 83 characters, the same length for every position. `kid` is the
//! first 4 bytes of an HKDF output of the master key, so a cursor minted
//! under another master key is `cursor_expired` (§5.4) even when the
//! configured `key_id` was left unchanged. The nonce is
//! `HMAC-SHA-256(k_nonce, 0x02 ‖ plain)[..12]` and is recomputed and compared
//! on decode (a synthetic-IV construction), so one plaintext always encodes
//! to the same cursor and the AEAD key never sees two plaintexts under one
//! nonce. The cursor carries a position, plus the feed head it was issued at
//! and that row's fingerprint (`acdp_registry_store::FeedRow::fingerprint`):
//! the registry refuses a cursor whose anchor row is no longer the row it
//! was, which expires every cursor a sequence reset could make skip
//! entries without operator action. The epoch (`[changes] cursor_epoch`)
//! covers the rest of §5.3's "every cursor issued before the reset": a
//! restore puts back the rows a cursor issued before the backup point
//! anchors on, so only a value outside the store can tell it apart, and
//! the restore runbook raises it. Nothing in it says who may see what, which is decided per
//! request, so a cursor replayed by another requester grants nothing (§5.5,
//! rec-004 E). The 6-byte header is bound into the AAD.
//!
//! A version-1 cursor (`0x01 ‖ key_id ‖ nonce12 ‖ GCM(seq_be8) ‖ tag16`, 51
//! characters, issued by 0.9 and earlier) has no anchor and cannot be placed after
//! a reset, so one that still opens is refused as expired, never served.
//!
//! **Secrets.** `nonce12 ‖ GCM(secret_utf8) ‖ tag16` under a random nonce,
//! with the subscription id in the AAD so a sealed secret cannot be moved
//! to another row.

use acdp_registry_types::{ChangesConfig, ReceiptConfig};
use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD};
use base64::Engine as _;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use ring::{hkdf, hmac};
use zeroize::Zeroize;

/// Required master key length.
pub const CHANGES_KEY_LEN: usize = 32;

const CURSOR_VERSION: u8 = 0x02;
const TAG_LEN: usize = 16;
const SEQ_LEN: usize = 8;
const FP_LEN: usize = 8;
const EPOCH_LEN: usize = 4;
const KID_LEN: usize = 4;
const HEADER_LEN: usize = 2 + KID_LEN;
const PLAIN_LEN: usize = SEQ_LEN + SEQ_LEN + FP_LEN + EPOCH_LEN;
const CURSOR_LEN: usize = HEADER_LEN + NONCE_LEN + PLAIN_LEN + TAG_LEN;
const CURSOR_AAD: &[u8] = b"acdp-registry/changes-cursor/v2";
const V1_VERSION: u8 = 0x01;
const V1_HEADER_LEN: usize = 2;
const V1_CURSOR_LEN: usize = V1_HEADER_LEN + NONCE_LEN + SEQ_LEN + TAG_LEN;
const V1_CURSOR_AAD: &[u8] = b"acdp-registry/changes-cursor/v1";
const SECRET_AAD: &[u8] = b"acdp-registry/subscription-secret/v1|";

const HKDF_SALT: &[u8] = b"acdp-registry/changes/v1";
const INFO_CURSOR_AEAD: &[u8] = b"acdp-registry/changes/cursor-aead/v1";
const INFO_CURSOR_NONCE: &[u8] = b"acdp-registry/changes/cursor-nonce/v1";
const INFO_SECRET: &[u8] = b"acdp-registry/changes/subscription-secret/v1";
const INFO_CURSOR_KID: &[u8] = b"acdp-registry/changes/cursor-kid/v1";

/// Why a master key was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangesKeyError {
    /// Not exactly [`CHANGES_KEY_LEN`] bytes.
    Length(usize),
    /// All zero: a placeholder, never a key.
    AllZero,
}

impl std::fmt::Display for ChangesKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Length(n) => write!(
                f,
                "the changes key must be exactly {CHANGES_KEY_LEN} bytes, got {n}"
            ),
            Self::AllZero => f.write_str("the changes key is all zero bytes"),
        }
    }
}

impl std::error::Error for ChangesKeyError {}

/// Why a presented cursor does not name a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorError {
    /// Not a cursor this registry issued: bad encoding, length, version,
    /// nonce or tag. Maps to `invalid_cursor`.
    Invalid,
    /// Well formed, but issued under a key id this registry no longer
    /// holds (a rotation). The consumer resyncs. Maps to `cursor_expired`.
    UnknownKey,
    /// A cursor this registry issued under another `cursor_epoch`, or a
    /// version-1 cursor, which carries no anchor. Neither can be placed
    /// after a reset. Maps to `cursor_expired`.
    Stale,
}

/// The feed head a cursor was issued at: a position and the fingerprint of
/// the row there (`acdp_registry_store::FeedHead::head_fp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorAnchor {
    pub seq: u64,
    pub fp: [u8; FP_LEN],
}

/// What a cursor names: the position to resume after, and its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedCursor {
    pub pos: u64,
    pub anchor: CursorAnchor,
}

/// Why a sealed secret did not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// Sealed under another key id; the subscription cannot be signed for.
    UnknownKey,
    /// Truncated, tampered, or bound to another subscription id.
    Corrupt,
    /// The system RNG failed while sealing.
    Rng,
}

impl std::fmt::Display for SealError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnknownKey => "sealed under a key id this registry does not hold",
            Self::Corrupt => "sealed secret does not open",
            Self::Rng => "system RNG failure",
        })
    }
}

impl std::error::Error for SealError {}

/// The derived RFC-ACDP-0018 keys. Holds no copy of the master key.
pub struct ChangesKey {
    key_id: u8,
    kid: [u8; KID_LEN],
    epoch: u32,
    cursor: LessSafeKey,
    nonce: hmac::Key,
    secret: LessSafeKey,
    rng: SystemRandom,
}

impl std::fmt::Debug for ChangesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangesKey")
            .field("key_id", &self.key_id)
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

struct Len32;

impl hkdf::KeyType for Len32 {
    fn len(&self) -> usize {
        32
    }
}

fn derive(prk: &hkdf::Prk, info: &'static [u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    prk.expand(&[info], Len32)
        .and_then(|okm| okm.fill(&mut out))
        .expect("HKDF-SHA-256 expands 32 bytes for any info");
    out
}

fn aead_key(mut bytes: [u8; 32]) -> LessSafeKey {
    let key = UnboundKey::new(&AES_256_GCM, &bytes).expect("AES-256-GCM takes a 32-byte key");
    bytes.zeroize();
    LessSafeKey::new(key)
}

impl ChangesKey {
    /// Derive the subkeys from `master`, issuing cursors under `key_id`.
    pub fn from_master(master: &[u8], key_id: u8) -> Result<Self, ChangesKeyError> {
        if master.len() != CHANGES_KEY_LEN {
            return Err(ChangesKeyError::Length(master.len()));
        }
        if master.iter().all(|b| *b == 0) {
            return Err(ChangesKeyError::AllZero);
        }
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, HKDF_SALT).extract(master);
        let mut nonce = derive(&prk, INFO_CURSOR_NONCE);
        let nonce_key = hmac::Key::new(hmac::HMAC_SHA256, &nonce);
        nonce.zeroize();
        let mut kid = [0u8; KID_LEN];
        kid.copy_from_slice(&derive(&prk, INFO_CURSOR_KID)[..KID_LEN]);
        Ok(Self {
            key_id,
            kid,
            epoch: 0,
            cursor: aead_key(derive(&prk, INFO_CURSOR_AEAD)),
            nonce: nonce_key,
            secret: aead_key(derive(&prk, INFO_SECRET)),
            rng: SystemRandom::new(),
        })
    }

    /// Issue cursors under `epoch`, refusing those of any other as stale.
    pub fn with_epoch(mut self, epoch: u32) -> Self {
        self.epoch = epoch;
        self
    }

    /// The key id cursors and sealed secrets are issued under.
    pub fn key_id(&self) -> u8 {
        self.key_id
    }

    fn header(&self) -> [u8; HEADER_LEN] {
        let mut h = [CURSOR_VERSION, self.key_id, 0, 0, 0, 0];
        h[2..].copy_from_slice(&self.kid);
        h
    }

    fn cursor_nonce(&self, version: u8, plain: &[u8]) -> [u8; NONCE_LEN] {
        let mut ctx = hmac::Context::with_key(&self.nonce);
        // Version 1 hashed the bare position; keep its input for decode.
        if version != V1_VERSION {
            ctx.update(&[version]);
        }
        ctx.update(plain);
        let tag = ctx.sign();
        let mut n = [0u8; NONCE_LEN];
        n.copy_from_slice(&tag.as_ref()[..NONCE_LEN]);
        n
    }

    fn cursor_aad(label: &[u8], header: &[u8]) -> Vec<u8> {
        let mut aad = Vec::with_capacity(label.len() + header.len());
        aad.extend_from_slice(label);
        aad.extend_from_slice(header);
        aad
    }

    /// The opaque cursor naming feed position `pos`, issued while the feed
    /// head was `anchor`.
    pub fn encode_cursor(&self, pos: u64, anchor: CursorAnchor) -> String {
        let header = self.header();
        let mut body = Vec::with_capacity(PLAIN_LEN + TAG_LEN);
        body.extend_from_slice(&pos.to_be_bytes());
        body.extend_from_slice(&anchor.seq.to_be_bytes());
        body.extend_from_slice(&anchor.fp);
        body.extend_from_slice(&self.epoch.to_be_bytes());
        let nonce = self.cursor_nonce(CURSOR_VERSION, &body);
        let mut out = Vec::with_capacity(CURSOR_LEN);
        out.extend_from_slice(&header);
        out.extend_from_slice(&nonce);
        self.cursor
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::cursor_aad(CURSOR_AAD, &header)),
                &mut body,
            )
            .expect("AES-256-GCM seals 28 bytes");
        out.extend_from_slice(&body);
        debug_assert_eq!(out.len(), CURSOR_LEN);
        URL_SAFE_NO_PAD.encode(out)
    }

    /// The position and anchor `cursor` names, if this registry issued it.
    pub fn decode_cursor(&self, cursor: &str) -> Result<DecodedCursor, CursorError> {
        let raw = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| CursorError::Invalid)?;
        let (aad_label, plain_len, header_len) = match (raw.first(), raw.len()) {
            (Some(&CURSOR_VERSION), CURSOR_LEN) => (CURSOR_AAD, PLAIN_LEN, HEADER_LEN),
            (Some(&V1_VERSION), V1_CURSOR_LEN) => (V1_CURSOR_AAD, SEQ_LEN, V1_HEADER_LEN),
            _ => return Err(CursorError::Invalid),
        };
        // A key id or master-key id this registry does not hold: issued here
        // under a rotated key, so expired rather than forged (§5.4).
        if raw[1] != self.key_id || (header_len == HEADER_LEN && raw[2..HEADER_LEN] != self.kid) {
            return Err(CursorError::UnknownKey);
        }
        let (header, rest) = raw.split_at(header_len);
        let (nonce, sealed) = rest.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| CursorError::Invalid)?;
        let mut sealed = sealed.to_vec();
        let plain = self
            .cursor
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::cursor_aad(aad_label, header)),
                &mut sealed,
            )
            .map_err(|_| CursorError::Invalid)?;
        if plain.len() != plain_len
            || !crate::secure_compare::ct_eq(&self.cursor_nonce(header[0], plain), &nonce)
        {
            return Err(CursorError::Invalid);
        }
        if header[0] == V1_VERSION {
            return Err(CursorError::Stale);
        }
        let word = |i: usize| {
            let mut b = [0u8; SEQ_LEN];
            b.copy_from_slice(&plain[i..i + SEQ_LEN]);
            u64::from_be_bytes(b)
        };
        let mut fp = [0u8; FP_LEN];
        fp.copy_from_slice(&plain[2 * SEQ_LEN..2 * SEQ_LEN + FP_LEN]);
        let mut epoch = [0u8; EPOCH_LEN];
        epoch.copy_from_slice(&plain[2 * SEQ_LEN + FP_LEN..]);
        if u32::from_be_bytes(epoch) != self.epoch {
            return Err(CursorError::Stale);
        }
        Ok(DecodedCursor {
            pos: word(0),
            anchor: CursorAnchor {
                seq: word(SEQ_LEN),
                fp,
            },
        })
    }

    fn secret_aad(subscription_id: &str) -> Vec<u8> {
        let mut aad = Vec::with_capacity(SECRET_AAD.len() + subscription_id.len());
        aad.extend_from_slice(SECRET_AAD);
        aad.extend_from_slice(subscription_id.as_bytes());
        aad
    }

    /// Seal `secret` for storage on subscription `subscription_id`:
    /// `nonce ‖ ciphertext ‖ tag`, stored beside [`Self::key_id`].
    pub fn seal_secret(&self, subscription_id: &str, secret: &str) -> Result<Vec<u8>, SealError> {
        let mut nonce = [0u8; NONCE_LEN];
        self.rng.fill(&mut nonce).map_err(|_| SealError::Rng)?;
        let mut body = secret.as_bytes().to_vec();
        self.secret
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::secret_aad(subscription_id)),
                &mut body,
            )
            .map_err(|_| SealError::Corrupt)?;
        let mut out = Vec::with_capacity(NONCE_LEN + body.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&body);
        body.zeroize();
        Ok(out)
    }

    /// Open a secret sealed by [`Self::seal_secret`] under `key_id`.
    pub fn open_secret(
        &self,
        subscription_id: &str,
        key_id: u8,
        sealed: &[u8],
    ) -> Result<String, SealError> {
        if key_id != self.key_id {
            return Err(SealError::UnknownKey);
        }
        if sealed.len() < NONCE_LEN + aead::MAX_TAG_LEN {
            return Err(SealError::Corrupt);
        }
        let (nonce, ct) = sealed.split_at(NONCE_LEN);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| SealError::Corrupt)?;
        let mut buf = ct.to_vec();
        let plain = self
            .secret
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(Self::secret_aad(subscription_id)),
                &mut buf,
            )
            .map_err(|_| SealError::Corrupt)?;
        let secret = String::from_utf8(plain.to_vec()).map_err(|_| SealError::Corrupt);
        buf.zeroize();
        secret
    }
}

/// Load the master key from `cfg` (exactly one of `key_b64` / `key_path`)
/// and derive the subkeys. Refuses a key equal to the receipt signing seed
/// in `receipt`: one secret must never serve two roles.
pub fn load_changes_key(
    cfg: &ChangesConfig,
    receipt: &ReceiptConfig,
) -> Result<ChangesKey, String> {
    let inline = cfg.key_b64.trim();
    let key_b64 = match (&cfg.key_path, inline.is_empty()) {
        (Some(_), false) => {
            return Err("changes.key_b64 and changes.key_path are both set; \
                        configure exactly one key source"
                .into());
        }
        (Some(path), true) => std::fs::read_to_string(path)
            .map_err(|e| format!("changes.key_path '{}': {e}", path.display()))?
            .trim()
            .to_string(),
        (None, false) => inline.to_string(),
        (None, true) => return Err("no changes key configured".into()),
    };
    let mut master = B64
        .decode(&key_b64)
        .map_err(|e| format!("changes key is not valid base64: {e}"))?;
    if let Ok(signing) = crate::receipt::load_signing_key(receipt) {
        if crate::secure_compare::ct_eq(&signing.seed_bytes(), &master) {
            master.zeroize();
            return Err("changes key equals the receipt signing seed; use a distinct key".into());
        }
    }
    let key = ChangesKey::from_master(&master, cfg.key_id)
        .map(|k| k.with_epoch(cfg.cursor_epoch))
        .map_err(|e| e.to_string());
    master.zeroize();
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8, id: u8) -> ChangesKey {
        ChangesKey::from_master(&[byte; 32], id).unwrap()
    }

    const A: CursorAnchor = CursorAnchor {
        seq: 9,
        fp: [0xa5; 8],
    };

    fn at(pos: u64) -> DecodedCursor {
        DecodedCursor { pos, anchor: A }
    }

    /// A version-1 cursor as 0.9 issued it: `GCM(seq_be8)` under the v1 AAD
    /// and the bare-position nonce.
    fn encode_v1(k: &ChangesKey, seq: u64) -> String {
        let header = [V1_VERSION, k.key_id];
        let nonce = k.cursor_nonce(V1_VERSION, &seq.to_be_bytes());
        let mut body = seq.to_be_bytes().to_vec();
        k.cursor
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(ChangesKey::cursor_aad(V1_CURSOR_AAD, &header)),
                &mut body,
            )
            .unwrap();
        let mut raw = header.to_vec();
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&body);
        URL_SAFE_NO_PAD.encode(raw)
    }

    fn cfg_with(key_b64: &str) -> ChangesConfig {
        ChangesConfig {
            enabled: true,
            key_b64: key_b64.into(),
            ..ChangesConfig::default()
        }
    }

    #[test]
    fn load_changes_key_needs_exactly_one_valid_distinct_source() {
        let receipt = ReceiptConfig {
            signing_key_seed_b64: B64.encode([5u8; 32]),
            ..ReceiptConfig::default()
        };
        let good = B64.encode([9u8; 32]);
        let loaded = load_changes_key(&cfg_with(&good), &receipt).unwrap();
        assert_eq!(
            loaded.decode_cursor(&key(9, 1).encode_cursor(3, A)),
            Ok(at(3))
        );

        let err = |c: &ChangesConfig| load_changes_key(c, &receipt).unwrap_err();
        assert!(err(&cfg_with("")).contains("no changes key"));
        assert!(err(&cfg_with("!!")).contains("base64"));
        assert!(err(&cfg_with(&B64.encode([9u8; 16]))).contains("32"));
        assert!(err(&cfg_with(&B64.encode([5u8; 32]))).contains("distinct"));
        let mut both = cfg_with(&good);
        both.key_path = Some("/nonexistent".into());
        assert!(err(&both).contains("both set"));
    }

    #[test]
    fn load_changes_key_reads_a_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("changes.key");
        std::fs::write(&path, format!("{}\n", B64.encode([9u8; 32]))).unwrap();
        let cfg = ChangesConfig {
            enabled: true,
            key_path: Some(path),
            ..ChangesConfig::default()
        };
        assert!(load_changes_key(&cfg, &ReceiptConfig::default()).is_ok());
        let missing = ChangesConfig {
            key_path: Some(dir.path().join("absent")),
            ..cfg
        };
        assert!(load_changes_key(&missing, &ReceiptConfig::default())
            .unwrap_err()
            .contains("key_path"));
    }

    #[test]
    fn master_key_must_be_32_nonzero_bytes() {
        assert_eq!(
            ChangesKey::from_master(&[7u8; 31], 1).unwrap_err(),
            ChangesKeyError::Length(31)
        );
        assert_eq!(
            ChangesKey::from_master(&[7u8; 33], 1).unwrap_err(),
            ChangesKeyError::Length(33)
        );
        assert_eq!(
            ChangesKey::from_master(&[0u8; 32], 1).unwrap_err(),
            ChangesKeyError::AllZero
        );
        let mut one_bit = [0u8; 32];
        one_bit[31] = 1;
        assert!(ChangesKey::from_master(&one_bit, 1).is_ok());
    }

    #[test]
    fn cursor_round_trips_every_position_shape() {
        let k = key(1, 1);
        for seq in [0, 1, 2, 255, 256, u32::MAX as u64, u64::MAX - 1, u64::MAX] {
            let anchor = CursorAnchor {
                seq: seq.saturating_add(3),
                fp: seq.to_le_bytes(),
            };
            let want = DecodedCursor { pos: seq, anchor };
            assert_eq!(
                k.decode_cursor(&k.encode_cursor(seq, anchor)),
                Ok(want),
                "seq {seq}"
            );
        }
    }

    #[test]
    fn a_cursor_from_another_epoch_is_stale() {
        let before = key(1, 1).with_epoch(4);
        let after = key(1, 1).with_epoch(5);
        let c = before.encode_cursor(3, A);
        assert_eq!(before.decode_cursor(&c), Ok(at(3)));
        assert_eq!(after.decode_cursor(&c), Err(CursorError::Stale));
        assert_ne!(c, after.encode_cursor(3, A));
        // Sealed, not a header byte: the epoch never shows in the clear.
        let raw = URL_SAFE_NO_PAD.decode(&c).unwrap();
        assert!(!raw.windows(4).any(|w| w == 4u32.to_be_bytes()));
    }

    #[test]
    fn load_changes_key_applies_the_cursor_epoch() {
        let cfg = ChangesConfig {
            cursor_epoch: 7,
            ..cfg_with(&B64.encode([9u8; 32]))
        };
        let loaded = load_changes_key(&cfg, &ReceiptConfig::default()).unwrap();
        let c = key(9, 1).with_epoch(7).encode_cursor(2, A);
        assert_eq!(loaded.decode_cursor(&c), Ok(at(2)));
        assert_eq!(
            loaded.decode_cursor(&key(9, 1).encode_cursor(2, A)),
            Err(CursorError::Stale)
        );
    }

    #[test]
    fn a_version_1_cursor_that_opens_is_stale() {
        let k = key(1, 1);
        assert_eq!(k.decode_cursor(&encode_v1(&k, 7)), Err(CursorError::Stale));
        // Not ours: still invalid, and a rotated key still unknown.
        assert_eq!(
            k.decode_cursor(&encode_v1(&key(2, 1), 7)),
            Err(CursorError::Invalid)
        );
        assert_eq!(
            k.decode_cursor(&encode_v1(&key(1, 2), 7)),
            Err(CursorError::UnknownKey)
        );
        let mut raw = URL_SAFE_NO_PAD.decode(encode_v1(&k, 7)).unwrap();
        raw[20] ^= 1;
        assert_eq!(
            k.decode_cursor(&URL_SAFE_NO_PAD.encode(raw)),
            Err(CursorError::Invalid)
        );
    }

    #[test]
    fn cursor_is_deterministic_and_fixed_length() {
        let k = key(1, 1);
        assert_eq!(k.encode_cursor(42, A), k.encode_cursor(42, A));
        assert_ne!(k.encode_cursor(42, A), k.encode_cursor(43, A));
        // The anchor is part of the cursor: same position, another head or
        // another row at that head, another cursor.
        let moved = CursorAnchor { seq: 10, ..A };
        let other_row = CursorAnchor { fp: [0; 8], ..A };
        assert_ne!(k.encode_cursor(4, A), k.encode_cursor(4, moved));
        assert_ne!(k.encode_cursor(4, A), k.encode_cursor(4, other_row));
        let lens: std::collections::BTreeSet<usize> = [0u64, 1, 9, 1 << 40, u64::MAX]
            .iter()
            .map(|s| k.encode_cursor(*s, A).len())
            .collect();
        assert_eq!(lens.into_iter().collect::<Vec<_>>(), vec![83]);
        const { assert!(83 <= acdp::types::change::MAX_CURSOR_LEN) };
        // The same position under the same key on another replica (a fresh
        // derivation) is byte-identical: no per-process state.
        assert_eq!(key(1, 1).encode_cursor(42, A), k.encode_cursor(42, A));
    }

    #[test]
    fn cursor_reveals_no_position_bytes() {
        let k = key(1, 1);
        let a = URL_SAFE_NO_PAD.decode(k.encode_cursor(1, A)).unwrap();
        let b = URL_SAFE_NO_PAD.decode(k.encode_cursor(2, A)).unwrap();
        // Only the 6-byte header is shared; every nonce/ciphertext/tag byte
        // differs between adjacent positions with overwhelming probability,
        // and none is the big-endian seq.
        assert_eq!(a[..HEADER_LEN], b[..HEADER_LEN]);
        assert!(!a.windows(8).any(|w| w == 1u64.to_be_bytes()));
        assert!(!a.windows(8).any(|w| w == A.seq.to_be_bytes() || w == A.fp));
        let same = a[HEADER_LEN..]
            .iter()
            .zip(&b[HEADER_LEN..])
            .filter(|(x, y)| x == y)
            .count();
        assert!(same < 8, "{same} of 56 body bytes coincide");
    }

    #[test]
    fn every_tampered_byte_is_invalid() {
        let k = key(1, 1);
        let raw = URL_SAFE_NO_PAD.decode(k.encode_cursor(7, A)).unwrap();
        for i in 0..raw.len() {
            for flip in [0x01u8, 0x80] {
                let mut t = raw.clone();
                t[i] ^= flip;
                let got = k.decode_cursor(&URL_SAFE_NO_PAD.encode(&t));
                let want = if (1..HEADER_LEN).contains(&i) {
                    CursorError::UnknownKey
                } else {
                    CursorError::Invalid
                };
                assert_eq!(got, Err(want), "byte {i} ^ {flip:#x}");
            }
        }
    }

    #[test]
    fn a_valid_ciphertext_under_a_wrong_nonce_is_invalid() {
        // The SIV check: re-seal one plaintext under another's nonce with the
        // real AEAD key — the tag verifies, only the nonce recomputation
        // refuses.
        let k = key(1, 1);
        let header = k.header();
        let plain = |pos: u64| {
            let mut p = pos.to_be_bytes().to_vec();
            p.extend_from_slice(&A.seq.to_be_bytes());
            p.extend_from_slice(&A.fp);
            p.extend_from_slice(&0u32.to_be_bytes());
            p
        };
        let nonce = k.cursor_nonce(CURSOR_VERSION, &plain(8));
        let mut body = plain(7);
        k.cursor
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(ChangesKey::cursor_aad(CURSOR_AAD, &header)),
                &mut body,
            )
            .unwrap();
        let mut raw = header.to_vec();
        raw.extend_from_slice(&nonce);
        raw.extend_from_slice(&body);
        assert_eq!(
            k.decode_cursor(&URL_SAFE_NO_PAD.encode(raw)),
            Err(CursorError::Invalid)
        );
    }

    #[test]
    fn foreign_and_malformed_cursors() {
        let k = key(1, 1);
        // Another master key under the same key id (a rotation that kept
        // `key_id`, or another deployment): its kid differs, so expired.
        assert_eq!(
            k.decode_cursor(&key(2, 1).encode_cursor(7, A)),
            Err(CursorError::UnknownKey)
        );
        // Our header on a body sealed under another master key: tag fails.
        let mut forged = URL_SAFE_NO_PAD
            .decode(key(2, 1).encode_cursor(7, A))
            .unwrap();
        forged[..HEADER_LEN].copy_from_slice(&k.header());
        assert_eq!(
            k.decode_cursor(&URL_SAFE_NO_PAD.encode(forged)),
            Err(CursorError::Invalid)
        );
        // Another master key, same configured key id: still one this
        // registry issued under a rotated key, so expired (§5.4).
        assert_ne!(key(2, 1).kid, k.kid);
        // Our key id rotated away: expired, not invalid.
        assert_eq!(
            k.decode_cursor(&key(1, 2).encode_cursor(7, A)),
            Err(CursorError::UnknownKey)
        );
        let good = k.encode_cursor(7, A);
        for bad in [
            "",
            "head",
            "not base64 !",
            &good[..82],
            &format!("{good}A"),
            &format!("{good}="),
            &good.replace(|c: char| c.is_ascii_alphanumeric(), "+"),
        ] {
            assert_eq!(k.decode_cursor(bad), Err(CursorError::Invalid), "{bad:?}");
        }
        // A version byte that does not match the length is not a cursor.
        for version in [0x00, 0x01, 0x03, 0xff] {
            let mut other = URL_SAFE_NO_PAD.decode(&good).unwrap();
            other[0] = version;
            assert_eq!(
                k.decode_cursor(&URL_SAFE_NO_PAD.encode(other)),
                Err(CursorError::Invalid),
                "version {version:#x}"
            );
        }
    }

    #[test]
    fn subkeys_are_independent() {
        // The secret seal and the cursor AEAD are different keys: a cursor
        // body does not open as a sealed secret and vice versa.
        let k = key(3, 1);
        let raw = URL_SAFE_NO_PAD.decode(k.encode_cursor(5, A)).unwrap();
        assert_eq!(
            k.open_secret("x", 1, &raw[HEADER_LEN..]),
            Err(SealError::Corrupt)
        );
        let sealed = k.seal_secret("x", "s3cret").unwrap();
        let mut as_cursor = k.header().to_vec();
        as_cursor.extend_from_slice(&sealed);
        assert!(k.decode_cursor(&URL_SAFE_NO_PAD.encode(as_cursor)).is_err());
    }

    #[test]
    fn secret_round_trips_and_binds_its_subscription() {
        let k = key(4, 9);
        let secret = "whsec_0123456789abcdef0123456789abcdef0123456789abcdef";
        let sealed = k.seal_secret("sub-1", secret).unwrap();
        assert_eq!(sealed.len(), NONCE_LEN + secret.len() + TAG_LEN);
        assert!(!sealed.windows(8).any(|w| w == &secret.as_bytes()[..8]));
        assert_eq!(k.open_secret("sub-1", 9, &sealed).unwrap(), secret);
        // Random nonce: sealing twice differs, both open.
        let again = k.seal_secret("sub-1", secret).unwrap();
        assert_ne!(sealed, again);
        assert_eq!(k.open_secret("sub-1", 9, &again).unwrap(), secret);
        // Moved to another row, other key id, another key, tampered, short.
        assert_eq!(k.open_secret("sub-2", 9, &sealed), Err(SealError::Corrupt));
        assert_eq!(
            k.open_secret("sub-1", 8, &sealed),
            Err(SealError::UnknownKey)
        );
        assert_eq!(
            key(5, 9).open_secret("sub-1", 9, &sealed),
            Err(SealError::Corrupt)
        );
        for i in 0..sealed.len() {
            let mut t = sealed.clone();
            t[i] ^= 1;
            assert_eq!(k.open_secret("sub-1", 9, &t), Err(SealError::Corrupt));
        }
        assert_eq!(
            k.open_secret("sub-1", 9, &sealed[..NONCE_LEN + TAG_LEN - 1]),
            Err(SealError::Corrupt)
        );
    }

    #[test]
    fn debug_never_prints_key_material() {
        let s = format!("{:?}", key(6, 3));
        assert_eq!(s, "ChangesKey { key_id: 3, epoch: 0, .. }");
    }
}
