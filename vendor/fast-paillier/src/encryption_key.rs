use rand_core::{CryptoRng, RngCore};
use rug::{Complete, Integer};

use crate::{utils, Ciphertext, Nonce, Plaintext};
use crate::{Bug, Error, Reason};

/// Paillier encryption key
#[derive(Clone, Debug)]
pub struct EncryptionKey {
    n: Integer,
    nn: Integer,
    half_n: Integer,
    neg_half_n: Integer,
}

impl EncryptionKey {
    /// Constructs an encryption key from `N`
    pub fn from_n(n: Integer) -> Self {
        let nn = n.clone() * &n;
        let half_n = n.clone() >> 1u32;
        let neg_half_n = -half_n.clone();
        Self {
            n,
            nn,
            half_n,
            neg_half_n,
        }
    }

    /// Returns `N`
    pub fn n(&self) -> &Integer {
        &self.n
    }

    /// Returns `N^2`
    pub fn nn(&self) -> &Integer {
        &self.nn
    }

    /// Returns `N/2`
    pub fn half_n(&self) -> &Integer {
        &self.half_n
    }

    /// `l(x) = (x-1)/n`
    pub(crate) fn l(&self, x: &Integer) -> Option<Integer> {
        if (x % self.n()).complete() != *Integer::ONE {
            return None;
        }
        if !utils::in_mult_group(x, self.nn()) {
            return None;
        }

        // (x - 1) / N
        Some((x - Integer::ONE).complete() / self.n())
    }

    /// Encrypts the plaintext `x` in `{-N/2, .., N_2}` with `nonce` in `Z*_n`
    ///
    /// Returns error if inputs are not in specified range
    pub fn encrypt_with(&self, x: &Plaintext, nonce: &Nonce) -> Result<Ciphertext, Error> {
        if !self.in_signed_group(x) || !utils::in_mult_group(nonce, self.n()) {
            return Err(Reason::Encrypt.into());
        }

        let x = if x.cmp0().is_ge() {
            x.clone()
        } else {
            (x + self.n()).complete()
        };

        // a = (1 + N)^x mod N^2 = (1 + xN) mod N^2
        let a = (Integer::ONE + (&x * self.n()).complete()) % self.nn();
        // b = nonce^N mod N^2
        let b = nonce
            .clone()
            .pow_mod(self.n(), self.nn())
            .map_err(|_| Bug::PowModUndef)?;

        let c = (a * b).modulo(self.nn());
        Ok(c)
    }

    /// Encrypts the plaintext `x` in `{-N/2, .., N_2}`
    ///
    /// Nonce is sampled randomly using `rng`.
    ///
    /// Returns error if plaintext is not in specified range
    pub fn encrypt_with_random(
        &self,
        rng: &mut (impl RngCore + CryptoRng),
        x: &Plaintext,
    ) -> Result<(Ciphertext, Nonce), Error> {
        let nonce = utils::sample_in_mult_group(rng, self.n());
        let ciphertext = self.encrypt_with(x, &nonce)?;
        Ok((ciphertext, nonce))
    }

    /// Homomorphic addition of two ciphertexts
    ///
    /// ```text
    /// oadd(Enc(a1), Enc(a2)) = Enc(a1 + a2)
    /// ```
    pub fn oadd(&self, c1: &Ciphertext, c2: &Ciphertext) -> Result<Ciphertext, Error> {
        if !utils::in_mult_group(c1, self.nn()) || !utils::in_mult_group(c2, self.nn()) {
            return Err(Reason::Ops.into());
        }
        Ok((c1 * c2).complete() % self.nn())
    }

    /// Homomorphic subtraction of two ciphertexts
    ///
    /// ```text
    /// osub(Enc(a1), Enc(a2)) = Enc(a1 - a2)
    /// ```
    pub fn osub(&self, c1: &Ciphertext, c2: &Ciphertext) -> Result<Ciphertext, Error> {
        if !utils::in_mult_group(c1, self.nn()) {
            return Err(Reason::Ops.into());
        }
        let c2 = self.oneg(c2)?;
        Ok((c1 * c2) % self.nn())
    }

    /// Homomorphic multiplication of scalar at ciphertext
    ///
    /// ```text
    /// omul(a, Enc(c)) = Enc(a * c)
    /// ```
    pub fn omul(&self, scalar: &Integer, ciphertext: &Ciphertext) -> Result<Ciphertext, Error> {
        if !utils::in_mult_group_abs(scalar, self.n())
            || !utils::in_mult_group(ciphertext, self.nn())
        {
            return Err(Reason::Ops.into());
        }

        Ok(ciphertext
            .pow_mod_ref(scalar, self.nn())
            .ok_or(Reason::Ops)?
            .into())
    }

    /// Homomorphic negation of a ciphertext
    ///
    /// ```text
    /// oneg(Enc(a)) = Enc(-a)
    /// ```
    pub fn oneg(&self, ciphertext: &Ciphertext) -> Result<Ciphertext, Error> {
        Ok(ciphertext.invert_ref(self.nn()).ok_or(Reason::Ops)?.into())
    }

    /// Checks whether `x` is `{-N/2, .., N/2}`
    pub fn in_signed_group(&self, x: &Integer) -> bool {
        self.neg_half_n <= *x && *x <= self.half_n
    }
}
