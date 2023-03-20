use aucpace::Database;
use curve25519_dalek::RistrettoPoint;
use password_hash::{ParamsString, SaltString};

/// Password Verifier database which can store the info for one user
#[derive(Debug, Default)]
pub struct SingleUserDatabase<const USERSIZE: usize> {
    user: Option<([u8; USERSIZE], usize)>,
    data: Option<(RistrettoPoint, SaltString, ParamsString)>,
}

impl<const USERSIZE: usize> Database for SingleUserDatabase<USERSIZE> {
    type PasswordVerifier = RistrettoPoint;

    fn lookup_verifier(
        &self,
        username: &[u8],
    ) -> Option<(Self::PasswordVerifier, SaltString, ParamsString)> {
        match self.user {
            Some((ref stored_username, len)) if &stored_username[..len] == username => {
                self.data.clone()
            }
            _ => None,
        }
    }

    fn store_verifier(
        &mut self,
        username: &[u8],
        salt: SaltString,
        // we don't care about this for an example
        _uad: Option<&[u8]>,
        verifier: Self::PasswordVerifier,
        params: ParamsString,
    ) {
        // silently fail because this is just an example and I'm lazy
        if username.len() <= USERSIZE {
            let mut buf = [0u8; USERSIZE];
            buf[..username.len()].copy_from_slice(username);
            self.user = Some((buf, username.len()));
            self.data = Some((verifier, salt, params));
        }
    }
}
