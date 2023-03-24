#[cfg(not(feature = "strong"))]
use aucpace::Database;

#[cfg(not(feature = "strong"))]
mod conditional_imports {
    pub use password_hash::SaltString;

    // normal AuCPace uses a raw salt string to store the salt
    pub type DbSalt = SaltString;
}

#[cfg(feature = "strong")]
mod conditional_imports {
    pub use aucpace::StrongDatabase;
    pub use curve25519_dalek::Scalar;

    // Strong AuCPace uses a blinded Scalar to store the salt
    pub type DbSalt = Scalar;
}

use conditional_imports::*;
use curve25519_dalek::RistrettoPoint;
use password_hash::ParamsString;

/// Password Verifier database which can store the info for one user
#[derive(Debug, Default)]
pub struct SingleUserDatabase<const USERSIZE: usize> {
    user: Option<([u8; USERSIZE], usize)>,
    data: Option<(RistrettoPoint, DbSalt, ParamsString)>,
}

#[cfg(not(feature = "strong"))]
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

#[cfg(feature = "strong")]
impl<const USERSIZE: usize> StrongDatabase for SingleUserDatabase<USERSIZE> {
    type PasswordVerifier = RistrettoPoint;
    type Exponent = Scalar;

    fn lookup_verifier_strong(
        &self,
        username: &[u8],
    ) -> Option<(Self::PasswordVerifier, Self::Exponent, ParamsString)> {
        match self.user {
            Some((ref stored_username, len)) if &stored_username[..len] == username => {
                self.data.clone()
            }
            _ => None,
        }
    }

    fn store_verifier_strong(
        &mut self,
        username: &[u8],
        _uad: Option<&[u8]>,
        verifier: Self::PasswordVerifier,
        secret_exponent: Self::Exponent,
        params: ParamsString,
    ) {
        // silently fail because this is just an example and I'm lazy
        if username.len() <= USERSIZE {
            let mut buf = [0u8; USERSIZE];
            buf[..username.len()].copy_from_slice(username);
            self.user = Some((buf, username.len()));
            self.data = Some((verifier, secret_exponent, params));
        }
    }
}
