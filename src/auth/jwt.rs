use anyhow::Result;
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub username: String,
    pub role: String,
    pub exp: usize,  // expiry timestamp
    pub iat: usize,  // issued at
    pub jti: String, // JWT ID — unique per token, used for blacklisting on logout
}

pub fn generate(
    user_id: &str,
    username: &str,
    role: &str,
    secret: &str,
    expiry_hours: u64,
) -> Result<String> {
    let now = Utc::now().timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        exp: now + (expiry_hours as usize * 3600),
        iat: now,
        jti: Uuid::new_v4().to_string(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn verify(token: &str, secret: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-for-unit-tests-only";

    #[test]
    fn test_generate_and_verify_token() {
        let token = generate("user-123", "admin", "super_admin", TEST_SECRET, 24)
            .expect("Should generate token");
        assert!(!token.is_empty());

        let claims = verify(&token, TEST_SECRET).expect("Should verify valid token");
        assert_eq!(claims.sub, "user-123");
        assert_eq!(claims.username, "admin");
        assert_eq!(claims.role, "super_admin");
    }

    #[test]
    fn test_verify_wrong_secret_fails() {
        let token =
            generate("user-123", "admin", "admin", TEST_SECRET, 24).expect("Should generate token");

        let result = verify(&token, "wrong-secret");
        assert!(
            result.is_err(),
            "Verification with wrong secret should fail"
        );
    }

    #[test]
    fn test_verify_malformed_token_fails() {
        let result = verify("not.a.valid.jwt", TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_empty_token_fails() {
        let result = verify("", TEST_SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_claims_contain_correct_role() {
        let token =
            generate("u1", "operator", "operator", TEST_SECRET, 1).expect("Should generate token");
        let claims = verify(&token, TEST_SECRET).expect("Should verify");
        assert_eq!(claims.role, "operator");
    }

    #[test]
    fn test_jti_is_unique_per_token() {
        let t1 = generate("u1", "admin", "admin", TEST_SECRET, 1).expect("token 1");
        let t2 = generate("u1", "admin", "admin", TEST_SECRET, 1).expect("token 2");
        let c1 = verify(&t1, TEST_SECRET).expect("claims 1");
        let c2 = verify(&t2, TEST_SECRET).expect("claims 2");
        assert!(!c1.jti.is_empty(), "jti should not be empty");
        assert_ne!(c1.jti, c2.jti, "each token should have a unique jti");
    }

    #[test]
    fn test_token_expiry_set_correctly() {
        use chrono::Utc;
        let before = Utc::now().timestamp() as usize;
        let token =
            generate("u1", "admin", "admin", TEST_SECRET, 2).expect("Should generate token");
        let after = Utc::now().timestamp() as usize;

        let claims = verify(&token, TEST_SECRET).expect("Should verify");
        // exp should be approximately now + 2*3600
        let expected_min = before + 2 * 3600;
        let expected_max = after + 2 * 3600;
        assert!(
            claims.exp >= expected_min,
            "exp {} < expected_min {}",
            claims.exp,
            expected_min
        );
        assert!(
            claims.exp <= expected_max,
            "exp {} > expected_max {}",
            claims.exp,
            expected_max
        );
    }
}
