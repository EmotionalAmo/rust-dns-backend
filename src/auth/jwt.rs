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

    // -------------------------------------------------------------------------
    // RBAC 权限矩阵测试
    // -------------------------------------------------------------------------

    /// 模拟 AdminUser extractor 的权限检查逻辑（镜像自 rbac.rs）
    fn check_admin_permission(role: &str) -> bool {
        matches!(role, "admin" | "super_admin")
    }

    // 测试 1：各角色 JWT claims 中 role 字段正确保留

    #[test]
    fn test_role_preserved_super_admin() {
        let token = generate("user-1", "su", "super_admin", TEST_SECRET, 1).expect("token");
        let claims = verify(&token, TEST_SECRET).expect("claims");
        assert_eq!(claims.role, "super_admin");
    }

    #[test]
    fn test_role_preserved_admin() {
        let token = generate("user-1", "adm", "admin", TEST_SECRET, 1).expect("token");
        let claims = verify(&token, TEST_SECRET).expect("claims");
        assert_eq!(claims.role, "admin");
    }

    #[test]
    fn test_role_preserved_operator() {
        let token = generate("user-1", "op", "operator", TEST_SECRET, 1).expect("token");
        let claims = verify(&token, TEST_SECRET).expect("claims");
        assert_eq!(claims.role, "operator");
    }

    #[test]
    fn test_role_preserved_read_only() {
        let token = generate("user-1", "ro", "read_only", TEST_SECRET, 1).expect("token");
        let claims = verify(&token, TEST_SECRET).expect("claims");
        assert_eq!(claims.role, "read_only");
    }

    // 测试 2：RBAC 权限矩阵逻辑（纯函数，不需要 HTTP）

    #[test]
    fn test_rbac_super_admin_has_admin_permission() {
        assert!(
            check_admin_permission("super_admin"),
            "super_admin should pass AdminUser check"
        );
    }

    #[test]
    fn test_rbac_admin_has_admin_permission() {
        assert!(
            check_admin_permission("admin"),
            "admin should pass AdminUser check"
        );
    }

    #[test]
    fn test_rbac_operator_lacks_admin_permission() {
        assert!(
            !check_admin_permission("operator"),
            "operator should be rejected by AdminUser check"
        );
    }

    #[test]
    fn test_rbac_read_only_lacks_admin_permission() {
        assert!(
            !check_admin_permission("read_only"),
            "read_only should be rejected by AdminUser check"
        );
    }

    // 测试 3：Token claims 与 RBAC 联合测试（完整矩阵）

    #[test]
    fn test_rbac_matrix_all_roles() {
        let roles_and_expected = [
            ("super_admin", true),
            ("admin", true),
            ("operator", false),
            ("read_only", false),
        ];

        for (role, expected_admin_access) in roles_and_expected {
            let token = generate("user-1", "testuser", role, TEST_SECRET, 1)
                .expect("Should generate token");
            let claims = verify(&token, TEST_SECRET).expect("Should verify");
            assert_eq!(claims.role, role, "Role should be preserved in token");
            let has_access = check_admin_permission(&claims.role);
            assert_eq!(
                has_access, expected_admin_access,
                "Role '{}' admin access should be {}",
                role, expected_admin_access
            );
        }
    }

    // 测试 4：防止角色混淆（大小写敏感 + 空格绕过）

    #[test]
    fn test_rbac_no_role_confusion() {
        // "ADMIN"（大写）不能绕过权限检查
        let token = generate("user-1", "hacker", "ADMIN", TEST_SECRET, 1).expect("token");
        let claims = verify(&token, TEST_SECRET).expect("claims");
        assert!(
            !check_admin_permission(&claims.role),
            "ADMIN (uppercase) should not get admin access"
        );

        // " admin"（前置空格）不能绕过
        let token2 = generate("user-1", "hacker", " admin", TEST_SECRET, 1).expect("token");
        let claims2 = verify(&token2, TEST_SECRET).expect("claims");
        assert!(
            !check_admin_permission(&claims2.role),
            "' admin' (with leading space) should not get admin access"
        );
    }
}
