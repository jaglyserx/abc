use secp256k1::{PublicKey, SecretKey, generate_keypair, rand};
use sha3::{
    Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Address([u8; 32]);

impl Address {
    pub fn from_public_key(pubkey: &PublicKey) -> Self {
        let pk_bytes = pubkey.serialize();

        let mut hasher = Shake256::default();
        hasher.update(&pk_bytes);

        let mut out = [0u8; 64];
        hasher.finalize_xof().read(&mut out);

        Address(out[..32].try_into().unwrap())
    }
}

#[derive(Debug)]
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

    fn write(self, dir: &str, pass: &str) {
        let bytes = self.prv.secret_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::PublicKey;

    #[test]
    fn account_address_matches_secret_key() {
        let acc = Account::new();

        let secp = secp256k1::Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &acc.prv);

        let expected_addr = Address::from_public_key(&pubkey);
        assert_eq!(acc.addr, expected_addr);
    }

    #[test]
    fn new_accounts_are_distinct() {
        let a1 = Account::new();
        let a2 = Account::new();

        assert_ne!(a1.prv, a2.prv);
        assert_ne!(a1.addr, a2.addr);
    }

    #[test]
    fn address_is_stable_for_same_public_key() {
        let acc = Account::new();

        let secp = secp256k1::Secp256k1::new();
        let pubkey = PublicKey::from_secret_key(&secp, &acc.prv);

        let addr1 = Address::from_public_key(&pubkey);
        let addr2 = Address::from_public_key(&pubkey);

        assert_eq!(addr1, addr2);
    }
}
