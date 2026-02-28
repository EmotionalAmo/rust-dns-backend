use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

pub fn hash(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("Password hashing failed: {}", e))?;
    Ok(hash.to_string())
}

pub fn verify(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_verify_correct_password() {
        let password = "secure-password-123";
        let hash = hash(password).expect("Should hash password");
        assert!(!hash.is_empty());
        assert!(verify(password, &hash), "Correct password should verify");
    }

    #[test]
    fn test_wrong_password_fails_verification() {
        let hash = hash("correct-password").expect("Should hash");
        assert!(
            !verify("wrong-password", &hash),
            "Wrong password should fail"
        );
    }

    #[test]
    fn test_two_hashes_of_same_password_differ() {
        // Argon2 使用随机 salt，同一密码的两个 hash 应不同
        let h1 = hash("same-password").expect("Should hash");
        let h2 = hash("same-password").expect("Should hash");
        assert_ne!(
            h1, h2,
            "Two hashes of same password should differ due to random salt"
        );
        // 但两者都应能验证原始密码
        assert!(verify("same-password", &h1));
        assert!(verify("same-password", &h2));
    }

    #[test]
    fn test_empty_password_hashes_and_verifies() {
        // 空密码在技术上是有效的（业务层负责拒绝它）
        let h = hash("").expect("Should hash empty string");
        assert!(verify("", &h));
        assert!(!verify("notempty", &h));
    }

    #[test]
    fn test_malformed_hash_returns_false() {
        assert!(!verify("any-password", "not-a-valid-hash-string"));
        assert!(!verify("any-password", ""));
    }

    #[test]
    fn test_hash_output_is_argon2_format() {
        let h = hash("test").expect("Should hash");
        // Argon2 PHC 格式以 $argon2id$ 开头
        assert!(
            h.starts_with("$argon2"),
            "Hash should be in PHC format, got: {}",
            h
        );
    }
}
