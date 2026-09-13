# did:web test fixture — a two-certificate chain for `agents.test`

**These are test-only credentials. The private key here is not a secret and never was.**

Three files, and the split between them is the whole point:

| file | role | `CA` | who uses it |
|---|---|---|---|
| `agents-test-ca.pem` | trust root | `CA:TRUE, pathlen:0` | handed to `WebResolver::with_test_endpoint` as the trusted root |
| `agents-test-cert.pem` | leaf, `SAN=DNS:agents.test`, `serverAuth` | `CA:FALSE` | presented by the fixture HTTPS server |
| `agents-test-key.pem` | the leaf's private key | — | the fixture HTTPS server |

ECDSA P-256 throughout. `tests/didweb/mod.rs` uses them to stand up an in-process HTTPS server serving
a producer's `did.json`, so a `did:web`-signed lifecycle event can actually be resolved in a test.

## Why two certificates and not one

**Because one does not work, and the reason is worth writing down.** The first version of this fixture
was a single self-signed `CA:TRUE` certificate doing both jobs — served as the leaf *and* handed to the
resolver as the trusted root. TLS rejected it:

```
invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))
```

rustls will not accept a CA certificate as an end-entity, so "make it a CA so it can also be the trust
root" is not a shortcut, it is a dead end. And `with_test_endpoint(pem, host, target)` takes a *single
root*, so the root cannot simply be omitted either. The fixture therefore has to be what a real
deployment has: a CA, and a leaf it signed.

An earlier revision of this file asserted the opposite — that `CA:TRUE` was load-bearing because a root
must be a CA — and even predicted the error a leaf-only cert would give. Half of that is true (a root
must be a CA); the conclusion drawn from it was wrong, because it ignored that the *same* certificate
was also the leaf. Left recorded here because a confident, plausible, wrong explanation in a fixture
README costs the next person more than no explanation at all.

## Why a committed fixture rather than generating one at runtime

Generating a certificate in-process needs `rcgen`, which is **not in `Cargo.lock`**. Adding it would
mean a new dev-dependency and lock churn for files that never change. So they are authored once, here,
and committed.

## Why this cannot be mistaken for a real credential

`.test` is **reserved by IANA** (RFC 2606 / RFC 6761) precisely so that it can never resolve on the
public internet. There is no `agents.test`, there never will be, and a certificate for it authenticates
nothing outside this test suite.

**A secret scanner may still flag `agents-test-key.pem`, because it is a real PEM private key.** That
is a true positive about the file type and a false positive about the risk. The fix is an allowlist
entry, *not* deleting the fixture — deleting it removes the only coverage of the `did:web` lifecycle
branch (the `LifecycleEventType::Retracted` arm of `lifecycle_transition`'s did:web dispatch, U-504's
accepted survivor), which is exactly what this exists to hold.

## Regenerating them

Not needed before **2126-08-20**. `didweb_fixture_certificates_are_not_near_expiry` checks **both**
certificates by name and fails loudly long before then, rather than letting a lapse present as a
mystery TLS error. Replace them **together** — the leaf is signed by the CA, so a new CA invalidates
the old leaf.

```sh
# CA (trust root)
openssl ecparam -name prime256v1 -genkey -noout -out agents-test-ca-key.pem
openssl req -new -x509 -key agents-test-ca-key.pem -sha256 -days 36500 \
  -subj "/CN=ACDP test did:web CA" \
  -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out agents-test-ca.pem

# leaf for agents.test, signed by that CA
openssl ecparam -name prime256v1 -genkey -noout -out agents-test-key.pem
openssl req -new -key agents-test-key.pem -subj "/CN=agents.test" -out leaf.csr
cat > leaf.ext <<'EXT'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:agents.test
EXT
openssl x509 -req -in leaf.csr -CA agents-test-ca.pem -CAkey agents-test-ca-key.pem \
  -set_serial 1 -days 36500 -sha256 -extfile leaf.ext -out agents-test-cert.pem

# verify the chain BEFORE committing it, and do not commit the CA's private key
openssl verify -CAfile agents-test-ca.pem agents-test-cert.pem
rm agents-test-ca-key.pem leaf.csr leaf.ext
```

**The CA's private key is deliberately not committed.** Nothing in the test suite needs it — the leaf
is already signed — and its absence means the fixture CA cannot be used to mint a certificate for any
other name.
