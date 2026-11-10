//! Authentication core plus an isolated, auth-only inspection transport.
//! No application commands, global trust, TLS key logging or media URLs.

mod legacy_certificate;
#[cfg(test)]
mod legacy_tests;
mod proto;
mod transport;
#[cfg(test)]
#[allow(unused_imports)]
pub(super) use transport::current_track;
pub(super) use transport::inspect;
pub(crate) use transport::{run_runtime, verify_idle};

use prost::Message;
use rand::Rng;
use rustls_pki_types::{CertificateDer, UnixTime};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use webpki::{EndEntityCert, KeyUsage, anchor_from_trusted_cert};
use x509_parser::{extensions::ParsedExtension, prelude::*};

const NAMESPACE: &str = "urn:x-cast:com.google.cast.tp.deviceauth";
const MAX_MESSAGE: usize = 64 * 1024;
const MAX_CERT: usize = 16 * 1024;
const MAX_CRL: usize = 32 * 1024;
const AUDIO_POLICY: &str = "1.3.6.1.4.1.11129.2.5.2";
const RSA_SHA256: &dyn rustls_pki_types::SignatureVerificationAlgorithm =
    webpki::ring::RSA_PKCS1_2048_8192_SHA256;

type Result<T> = std::result::Result<T, Failure>;

// Only static categories may leave the verifier; never print peer-supplied fields.
#[derive(Debug, PartialEq, Eq)]
enum Failure {
    Bounds,
    Malformed,
    Envelope,
    Deadline,
    Nonce,
    Algorithm,
    TlsValidity,
    Chain,
    CertificateAlgorithm(&'static str),
    Signature,
    Revocation,
    Policy,
    Transport,
    Cancelled,
    Media(&'static str),
}

struct Trust<'a> {
    device: &'a [&'a [u8]],
    revocation: &'a [&'a [u8]],
    allow_legacy_device_chain: bool,
}

// Not Clone: consuming this context prevents reusing a completed challenge.
struct Context {
    peer: Vec<u8>,
    nonce: [u8; 16],
    started: Instant,
}

#[derive(Debug)]
struct Authenticated {
    peer_fingerprint: [u8; 32],
    audio_only: bool,
    valid_until: u64,
}

impl Context {
    fn new(peer_certificate: &[u8]) -> Result<Self> {
        certificate(peer_certificate)?;
        let mut nonce = [0; 16];
        rand::rng().fill(&mut nonce);
        Ok(Self {
            peer: peer_certificate.to_vec(),
            nonce,
            started: Instant::now(),
        })
    }

    fn challenge(&self) -> Vec<u8> {
        let auth = proto::Auth {
            challenge: Some(proto::Challenge {
                signature_algorithm: Some(1), // RSA PKCS#1 v1.5
                hash_algorithm: Some(1),      // SHA-256, never SHA-1 fallback
                nonce: Some(self.nonce.to_vec()),
            }),
            ..Default::default()
        };
        envelope(auth, "sender-0", "receiver-0").encode_to_vec()
    }

    fn authenticate(self, bytes: &[u8], trust: &Trust<'_>, now: u64) -> Result<Authenticated> {
        if self.started.elapsed() > Duration::from_secs(10) {
            return Err(Failure::Deadline);
        }
        bounded(bytes, MAX_MESSAGE)?;
        let message = proto::Envelope::decode(bytes).map_err(|_| Failure::Malformed)?;
        if message.version != Some(0)
            || message.source.as_deref() != Some("receiver-0")
            || message.destination.as_deref() != Some("sender-0")
            || message.namespace.as_deref() != Some(NAMESPACE)
            || message.payload_type != Some(1)
            || message.text.is_some()
            || message.continued.unwrap_or(false)
            || message.remaining.is_some()
        {
            return Err(Failure::Envelope);
        }
        let auth = proto::Auth::decode(message.binary.as_deref().ok_or(Failure::Malformed)?)
            .map_err(|_| Failure::Malformed)?;
        if auth.error.is_some() || auth.challenge.is_some() {
            return Err(Failure::Malformed);
        }
        let response = auth.response.ok_or(Failure::Malformed)?;
        if response.nonce.as_deref() != Some(self.nonce.as_slice()) {
            return Err(Failure::Nonce);
        }
        if response.signature_algorithm.unwrap_or(1) != 1 || response.hash_algorithm != Some(1) {
            return Err(Failure::Algorithm);
        }
        let peer = certificate(&self.peer)?;
        let mut valid_until = expiry(&peer, now).map_err(|_| Failure::TlsValidity)?;
        if valid_until.saturating_sub(now) > 4 * 24 * 60 * 60 {
            return Err(Failure::TlsValidity);
        }
        let leaf = response.certificate.as_deref().ok_or(Failure::Malformed)?;
        let legacy_algorithms = [RSA_SHA256, &legacy_certificate::LegacyCertificateRsaSha1];
        let strict_algorithms = [RSA_SHA256];
        let algorithms = if trust.allow_legacy_device_chain {
            &legacy_algorithms[..]
        } else {
            &strict_algorithms[..]
        };
        let chain = verify_chain_with_algorithms(
            leaf,
            &response.intermediates,
            trust.device,
            now,
            algorithms,
        )?;
        let mut signed = Vec::with_capacity(16 + self.peer.len());
        signed.extend_from_slice(&self.nonce);
        signed.extend_from_slice(&self.peer);
        verify_signature(
            leaf,
            &signed,
            response.signature.as_deref().ok_or(Failure::Malformed)?,
        )?;

        let crl = verify_crl(
            response.crl.as_deref().ok_or(Failure::Revocation)?,
            trust,
            now,
        )?;
        valid_until = valid_until.min(crl.not_after.ok_or(Failure::Revocation)?);
        check_revocation(&chain, &crl)?;
        let mut audio_only = false;
        for der in &chain {
            let cert = certificate(der)?;
            valid_until = valid_until.min(expiry(&cert, now)?);
            // Reject duplicate/invalid extensions instead of allowing policy ambiguity.
            cert.extensions_map().map_err(|_| Failure::Policy)?;
            for extension in cert.extensions() {
                match extension.parsed_extension() {
                    ParsedExtension::CertificatePolicies(policies) => {
                        audio_only |= policies
                            .iter()
                            .any(|p| p.policy_id.to_id_string() == AUDIO_POLICY);
                    }
                    ParsedExtension::ParseError { .. } => return Err(Failure::Policy),
                    _ => {}
                }
            }
        }
        // Parsing/path search is input-bounded, but still recheck the total budget.
        if self.started.elapsed() > Duration::from_secs(10) {
            return Err(Failure::Deadline);
        }
        Ok(Authenticated {
            peer_fingerprint: Sha256::digest(&self.peer).into(),
            audio_only,
            valid_until,
        })
    }
}

fn bounded(bytes: &[u8], max: usize) -> Result<()> {
    if bytes.is_empty() || bytes.len() > max {
        Err(Failure::Bounds)
    } else {
        Ok(())
    }
}

fn certificate(bytes: &[u8]) -> Result<X509Certificate<'_>> {
    bounded(bytes, MAX_CERT)?;
    let (rest, cert) = X509Certificate::from_der(bytes).map_err(|_| Failure::Malformed)?;
    if !rest.is_empty() {
        return Err(Failure::Malformed);
    }
    Ok(cert)
}

fn expiry(cert: &X509Certificate<'_>, now: u64) -> Result<u64> {
    let start =
        u64::try_from(cert.validity().not_before.timestamp()).map_err(|_| Failure::Chain)?;
    let end = u64::try_from(cert.validity().not_after.timestamp()).map_err(|_| Failure::Chain)?;
    if start > now || end < now || start > end {
        return Err(Failure::Chain);
    }
    Ok(end)
}

fn verify_chain(
    leaf: &[u8],
    intermediates: &[Vec<u8>],
    roots: &[&[u8]],
    now: u64,
) -> Result<Vec<Vec<u8>>> {
    verify_chain_with_algorithms(leaf, intermediates, roots, now, &[RSA_SHA256])
}

fn verify_chain_with_algorithms(
    leaf: &[u8],
    intermediates: &[Vec<u8>],
    roots: &[&[u8]],
    now: u64,
    algorithms: &[&dyn rustls_pki_types::SignatureVerificationAlgorithm],
) -> Result<Vec<Vec<u8>>> {
    if intermediates.len() > 4 || roots.is_empty() || roots.len() > 8 {
        return Err(Failure::Bounds);
    }
    let parsed = certificate(leaf)?;
    let usage = parsed
        .key_usage()
        .map_err(|_| Failure::Chain)?
        .ok_or(Failure::Chain)?;
    if !usage.value.digital_signature() {
        return Err(Failure::Chain);
    }
    for der in intermediates
        .iter()
        .map(Vec::as_slice)
        .chain(roots.iter().copied())
    {
        certificate(der)?;
    }
    let leaf_der = CertificateDer::from(leaf);
    let end = EndEntityCert::try_from(&leaf_der).map_err(|_| Failure::Chain)?;
    let root_ders: Vec<_> = roots.iter().map(|der| CertificateDer::from(*der)).collect();
    let anchors: Vec<_> = root_ders
        .iter()
        .map(anchor_from_trusted_cert)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| Failure::Chain)?;
    let intermediate_ders: Vec<_> = intermediates
        .iter()
        .map(|der| CertificateDer::from(der.as_slice()))
        .collect();
    let path = end
        .verify_for_usage(
            algorithms,
            &anchors,
            &intermediate_ders,
            UnixTime::since_unix_epoch(Duration::from_secs(now)),
            KeyUsage::client_auth(),
            None,
            None,
        )
        .map_err(|error| match error {
            webpki::Error::UnsupportedSignatureAlgorithmContext(context) => {
                Failure::CertificateAlgorithm(certificate_algorithm(
                    &context.signature_algorithm_id,
                ))
            }
            webpki::Error::UnsupportedSignatureAlgorithmForPublicKeyContext(context) => {
                Failure::CertificateAlgorithm(certificate_algorithm(
                    &context.signature_algorithm_id,
                ))
            }
            _ => Failure::Chain,
        })?;
    let mut chain = vec![leaf.to_vec()];
    chain.extend(
        path.intermediate_certificates()
            .map(|cert| cert.der().to_vec()),
    );
    let root_index = anchors
        .iter()
        .position(|anchor| anchor == path.anchor())
        .ok_or(Failure::Chain)?;
    chain.push(roots[root_index].to_vec());
    for der in &chain {
        expiry(&certificate(der)?, now)?;
    }
    Ok(chain)
}

fn certificate_algorithm(id: &[u8]) -> &'static str {
    use rustls_pki_types::alg_id;
    if id
        == [
            0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x05, 0x05, 0x00,
        ]
    {
        "Rejected certificate algorithm: RSA/SHA-1"
    } else if id == alg_id::RSA_PKCS1_SHA384.as_ref() {
        "Rejected certificate algorithm: RSA/SHA-384"
    } else if id == alg_id::RSA_PKCS1_SHA512.as_ref() {
        "Rejected certificate algorithm: RSA/SHA-512"
    } else if id == alg_id::RSA_PKCS1_SHA256.as_ref() {
        "Rejected certificate algorithm/key combination: RSA/SHA-256"
    } else if id == alg_id::ECDSA_SHA256.as_ref() {
        "Rejected certificate algorithm: ECDSA/SHA-256"
    } else {
        "Rejected unsupported certificate signature algorithm/key"
    }
}

fn verify_signature(cert: &[u8], message: &[u8], signature: &[u8]) -> Result<()> {
    if !(256..=1024).contains(&signature.len()) {
        return Err(Failure::Signature);
    }
    let der = CertificateDer::from(cert);
    EndEntityCert::try_from(&der)
        .map_err(|_| Failure::Chain)?
        .verify_signature(RSA_SHA256, message, signature)
        .map_err(|_| Failure::Signature)
}

fn verify_crl(bytes: &[u8], trust: &Trust<'_>, now: u64) -> Result<proto::Crl> {
    bounded(bytes, MAX_CRL)?;
    let bundle = proto::CrlBundle::decode(bytes).map_err(|_| Failure::Revocation)?;
    // Conservative subset: exactly one v0 list, never optional or first-valid-wins.
    if bundle.lists.len() != 1 {
        return Err(Failure::Revocation);
    }
    let signed = &bundle.lists[0];
    let tbs = signed.tbs.as_deref().ok_or(Failure::Revocation)?;
    bounded(tbs, MAX_CRL)?;
    let signer = signed.signer.as_deref().ok_or(Failure::Revocation)?;
    let chain =
        verify_chain(signer, &[], trust.revocation, now).map_err(|_| Failure::Revocation)?;
    verify_signature(
        signer,
        tbs,
        signed.signature.as_deref().ok_or(Failure::Revocation)?,
    )
    .map_err(|_| Failure::Revocation)?;
    let mut crl = proto::Crl::decode(tbs).map_err(|_| Failure::Revocation)?;
    let start = crl.not_before.ok_or(Failure::Revocation)?;
    let end = crl.not_after.ok_or(Failure::Revocation)?;
    if crl.version.unwrap_or(0) != 0
        || start > now
        || end < now
        || start > end
        || crl.revoked_keys.len() > 256
        || crl.revoked_ranges.len() > 256
        || crl.revoked_keys.iter().any(|hash| hash.len() != 32)
    {
        return Err(Failure::Revocation);
    }
    for range in &crl.revoked_ranges {
        if range.issuer.as_deref().is_none_or(|hash| hash.len() != 32)
            || range.first.is_none()
            || range.last.is_none()
            || range.first > range.last
        {
            return Err(Failure::Revocation);
        }
    }
    let mut expiry_time = end;
    for der in &chain {
        expiry_time = expiry_time.min(expiry(&certificate(der)?, now)?);
    }
    crl.not_after = Some(expiry_time);
    Ok(crl)
}

fn check_revocation(chain: &[Vec<u8>], crl: &proto::Crl) -> Result<()> {
    for (index, der) in chain.iter().enumerate() {
        let cert = certificate(der)?;
        let hash = Sha256::digest(cert.public_key().raw);
        if crl
            .revoked_keys
            .iter()
            .any(|revoked| revoked.as_slice() == hash.as_slice())
        {
            return Err(Failure::Revocation);
        }
        if index > 0 {
            let subordinate = certificate(&chain[index - 1])?;
            let raw = subordinate.raw_serial();
            if raw.is_empty() || raw[0] & 0x80 != 0 {
                return Err(Failure::Revocation);
            }
            let raw = raw.strip_prefix(&[0]).unwrap_or(raw);
            // Cast ranges apply only to nonnegative serials representable as u64.
            if raw.len() <= 8 {
                let serial = raw
                    .iter()
                    .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
                for range in &crl.revoked_ranges {
                    if range.issuer.as_deref() == Some(hash.as_slice())
                        && range.first.is_some_and(|first| serial >= first)
                        && range.last.is_some_and(|last| serial <= last)
                    {
                        return Err(Failure::Revocation);
                    }
                }
            }
        }
    }
    Ok(())
}

fn envelope(auth: proto::Auth, source: &str, destination: &str) -> proto::Envelope {
    proto::Envelope {
        version: Some(0),
        source: Some(source.into()),
        destination: Some(destination.into()),
        namespace: Some(NAMESPACE.into()),
        payload_type: Some(1),
        binary: Some(auth.encode_to_vec()),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
