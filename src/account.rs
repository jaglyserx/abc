use std::{fs::File, io::Write, path::PathBuf};

use aes_gcm::{
    AeadCore, Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, OsRng as OsRngAES},
};
use argon2::Argon2;
use rand::rngs::OsRng;
use secp256k1::{
    PublicKey, SecretKey, generate_keypair,
    rand::{self, TryRngCore},
};
use sha3::{
    Shake256,
    digest::{ExtendableOutput, Update, XofReader},
};

use crate::constants;

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

    fn write(self, dir: &str, pass: &str) -> anyhow::Result<()> {
        let bytes = self.prv.secret_bytes();
        let encrypted = encrypt(&bytes, pass.as_bytes())?;
        let credentials = str::from_utf8(&self.addr.0)?;
        let path = PathBuf::from(dir).join(credentials);
        let mut file = File::create(path)?;
        file.write_all(&encrypted)?;
        Ok(())
    }
}

fn encrypt(msg: &[u8; 32], pass: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut salt = [0u8; 32];
    OsRng.try_fill_bytes(&mut salt)?;

    let mut key = [0u8; 32];
    Argon2::default().hash_password_into(pass, &salt, &mut key)?;
    let key = key.try_into()?;

    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRngAES);
    let ciphertext = cipher
        .encrypt(&nonce, msg.as_slice())
        .map_err(|e| anyhow::anyhow!(e))?;

    let mut combined = Vec::with_capacity(32 + nonce.len() + salt.len());
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&nonce);
    combined.extend_from_slice(&ciphertext);
    Ok(combined)
}

fn decrypt(data: &[u8], pass: &[u8]) -> anyhow::Result<Vec<u8>> {
    let (salt, rest) = data.split_at(constants::SALT_SIZE);
    let (nonce, ciphertext) = rest.split_at(constants::NONCE_SIZE);

    let mut key = [0u8; 32];
    Argon2::default().hash_password_into(pass, &salt, &mut key)?;
    let key = key.try_into()?;

    let cipher = Aes256Gcm::new(&key);
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| anyhow::anyhow!(e))
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
