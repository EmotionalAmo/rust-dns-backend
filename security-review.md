# Security Review: Backend Vulnerability Audit

## Summary
- Critical: 0 | High: 0 | Medium: 2 | Low: 1

## Findings

### [MEDIUM] RSA Crate Vulnerability (Marvin Attack)
- **File:** `Cargo.lock` (dependency `rsa v0.9.10`)
- **OWASP:** A06 Vulnerable and Outdated Components
- **Description:** Cargo audit detected RUSTSEC-2023-0071 in the `rsa` crate (pulled by `sqlx-mysql` -> `sqlx`). The vulnerability (Marvin Attack) relates to potential key recovery through timing side-channels.
- **Recommendation:** No fixed upgrade is available currently for `rsa 0.9.10` in this branch. Monitor rustsec for updates or disable the `mysql` feature in `sqlx` if MySQL is not being used (the app appears to use SQLite, so removing `mysql` feature from `sqlx` in `Cargo.toml` would resolve this).

### [MEDIUM] Unmaintained `rustls-pemfile` Crate
- **File:** `Cargo.lock` (dependency `rustls-pemfile v1.0.4` and `v2.2.0`)
- **OWASP:** A06 Vulnerable and Outdated Components
- **Description:** Cargo audit flagged the `rustls-pemfile` crate as unmaintained (RUSTSEC-2025-0134). This crate is pulled in by `hickory-proto` and `hickory-resolver`.
- **Recommendation:** Update `hickory-dns` ecosystem dependencies to newer versions that have migrated away from the unmaintained `rustls-pemfile` crate, or monitor for standard library/rustls native replacements.

### [LOW] JWT Secret Hardcoded Expiration Default
- **File:** `src/api/handlers/auth.rs` / `src/auth/jwt.rs`
- **OWASP:** A07 Identification and Authentication Failures
- **Description:** JWT expiration is managed via `state.jwt_expiry_hours`. While this is configurable, there is no explicit check enforcing a maximum expiration limit in the code (e.g., preventing a configuration of 99999 hours).
- **Recommendation:** Enforce a hard maximum on JWT expiration times (e.g., maximum of 7 days / 168 hours) to ensure tokens cannot be valid indefinitely even if misconfigured.

## Passed Checks
- **A01 Broken Access Control:** Endpoints properly use `AuthUser` and `AdminUser` middleware extractors to enforce access control.
- **A02 Cryptographic Failures:** Passwords securely hashed with `argon2` with random salts.
- **A03 Injection:** No raw SQL injections found. All `sqlx::query` formatting is parameterized natively binding values. `Command::new` or `subprocess` usage is absent.
- **A04 Insecure Design:** Login endpoints appropriately use rate limiting (5 failures per 15 minutes) and default password warnings.
- **A05 Security Misconfiguration:** Not detected.
- **A10 Server-Side Request Forgery (SSRF):** Remote list fetching in `dns/subscription.rs` validates scheme (`http`/`https`) and strictly prevents fetching from private/loopback IP addresses (`127.0.0.0/8`, `10/8`, etc).
