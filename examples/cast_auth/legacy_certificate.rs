//! Opt-in device-certificate compatibility only. Never used for TLS handshakes,
//! fresh device-auth signatures, CRL signatures or CRL-authority paths.
use rustls_pki_types::{
    AlgorithmIdentifier, InvalidSignature, SignatureVerificationAlgorithm, alg_id,
};

#[derive(Debug)]
pub(super) struct LegacyCertificateRsaSha1;
impl SignatureVerificationAlgorithm for LegacyCertificateRsaSha1 {
    fn verify_signature(
        &self,
        key: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> std::result::Result<(), InvalidSignature> {
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY,
            key,
        )
        .verify(message, signature)
        .map_err(|_| InvalidSignature)
    }
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::RSA_ENCRYPTION
    }
    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        AlgorithmIdentifier::from_slice(&[
            0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x05, 0x05, 0x00,
        ])
    }
}
