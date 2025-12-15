use secp256k1::{PublicKey, SecretKey, generate_keypair, rand};
use sha3::{
    Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address([u8; 32]);

impl Address {
    pub fn from_public_key(pubkey: &PublicKey) -> Self {
        let pk_bytes = pubkey.serialize(); // [u8; 33]

        let mut hasher = Shake256::default();
        hasher.update(&pk_bytes);

        let mut out = [0u8; 64];
        hasher.finalize_xof().read(&mut out);

        Address(out[..32].try_into().unwrap())
    }
}

struct Account {
    prv: SecretKey,
    addr: Address,
}

impl Account {
    fn new() -> Account {
        let (secret_key, public_key) = generate_keypair(&mut rand::rng());
        let address = Address::from_public_key(&public_key);

        Account {
            prv: secret_key,
            addr: address,
        }
    }
}
