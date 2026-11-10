// Wire field definitions follow Chromium Open Screen's Cast protocol schemas.
// See tests/fixtures/cast-auth/OPENSCREEN-LICENSE and README.md for provenance.
// Optional fields retain presence so missing proto2 fields fail closed.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Envelope {
    #[prost(int32, optional, tag = "1")]
    pub version: Option<i32>,
    #[prost(string, optional, tag = "2")]
    pub source: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub destination: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub namespace: Option<String>,
    #[prost(int32, optional, tag = "5")]
    pub payload_type: Option<i32>,
    #[prost(string, optional, tag = "6")]
    pub text: Option<String>,
    #[prost(bytes = "vec", optional, tag = "7")]
    pub binary: Option<Vec<u8>>,
    #[prost(bool, optional, tag = "8")]
    pub continued: Option<bool>,
    #[prost(uint32, optional, tag = "9")]
    pub remaining: Option<u32>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Auth {
    #[prost(message, optional, tag = "1")]
    pub challenge: Option<Challenge>,
    #[prost(message, optional, tag = "2")]
    pub response: Option<Response>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub error: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Challenge {
    #[prost(int32, optional, tag = "1")]
    pub signature_algorithm: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub nonce: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "3")]
    pub hash_algorithm: Option<i32>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Response {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub signature: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub certificate: Option<Vec<u8>>,
    #[prost(bytes = "vec", repeated, tag = "3")]
    pub intermediates: Vec<Vec<u8>>,
    #[prost(int32, optional, tag = "4")]
    pub signature_algorithm: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "5")]
    pub nonce: Option<Vec<u8>>,
    #[prost(int32, optional, tag = "6")]
    pub hash_algorithm: Option<i32>,
    #[prost(bytes = "vec", optional, tag = "7")]
    pub crl: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct CrlBundle {
    #[prost(message, repeated, tag = "1")]
    pub lists: Vec<SignedCrl>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SignedCrl {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub tbs: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "2")]
    pub signer: Option<Vec<u8>>,
    #[prost(bytes = "vec", optional, tag = "3")]
    pub signature: Option<Vec<u8>>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Crl {
    #[prost(uint64, optional, tag = "1")]
    pub version: Option<u64>,
    #[prost(uint64, optional, tag = "2")]
    pub not_before: Option<u64>,
    #[prost(uint64, optional, tag = "3")]
    pub not_after: Option<u64>,
    #[prost(bytes = "vec", repeated, tag = "4")]
    pub revoked_keys: Vec<Vec<u8>>,
    #[prost(message, repeated, tag = "5")]
    pub revoked_ranges: Vec<SerialRange>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SerialRange {
    #[prost(bytes = "vec", optional, tag = "1")]
    pub issuer: Option<Vec<u8>>,
    #[prost(uint64, optional, tag = "2")]
    pub first: Option<u64>,
    #[prost(uint64, optional, tag = "3")]
    pub last: Option<u64>,
}
