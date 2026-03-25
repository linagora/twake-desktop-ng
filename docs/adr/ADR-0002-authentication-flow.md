# ADR-0002: OIDC PKCE Authentication Flow

## Status

Proposed

## Context

Twake Desktop NG is a sync application with an embedded web capability. We need to define an authentication flow that:

1. Enables simple initial configuration for the user
2. Supports automatic background authentication
3. Shares the same account between the sync application and web capability
4. Handles two usage types: interactive and non-interactive (scheduled tasks)

**Key points:**

- System: OAuth2 / OIDC
- First contact: discovery via `/.well-known/twake/desktop-configuration`
- Two token lifetimes: interactive (A) and non-interactive (B)
- PKCE for interactive flow security

## Decision

### 1. Initial Configuration

**Step 1: Server URL input**

- User enters the Twake server URL (e.g., `https://twake.company.com`)
- Application queries: `https://twake.company.com/.well-known/twake/desktop-configuration`

**Step 2: Configuration retrieval**
The `.well-known` response contains:

```json
{
  "sso_url": "https://sso.company.com",
  "client_id_interactive": "twake-desktop-interactive",
  "client_id_background": "twake-desktop-background",
  "scopes": ["openid", "profile", "offline_access"]
}
```

### 2. Interactive Authentication Flow

**Client used:** `client_id_interactive`

**Lifetime:** Refresh token valid for duration (A)

**OIDC PKCE flow:**

```
1. Application generates code_verifier and code_challenge
2. Redirect to SSO in embedded webview
3. User authenticates (if not already)
4. Callback to application with authorization code
5. Exchange code + code_verifier for tokens
6. Secure storage of tokens (access_token, refresh_token)
```

**Token endpoints:**

- Authorization: `{sso_url}/oauth2/auth`
- Token: `{sso_url}/oauth2/token`

### 3. Non-Interactive Authentication Flow (Scheduled Tasks)

**Client used:** `client_id_background` (if provided in .well-known)

**Lifetime:** Refresh token valid for duration (B), where B > A

**Token exchange:**

```
1. Application uses refresh_token from interactive flow
2. Exchange to client_id_background
3. Obtain new tokens with lifetime B
4. Use for scheduled tasks
```

**Trigger conditions:**

- **Automatic on interactive auth:** Whenever a successful interactive authentication occurs, if no valid background token exists, immediately trigger the token exchange to client_id_background
- **On-demand:** When a scheduled task needs to run and no valid background token is available

**If `client_id_background` is not provided:**

- Use the same `client_id_interactive` for all cases
- Lifetime will be configured for this client

### 4. Token Sharing

**Same account, same session:**

- Tokens are shared between sync application and web capability
- Web capability can use the same tokens for APIs
- Native Single Sign-On via OIDC

### 5. Token Refresh

**Strategy:**

```
- Auto-refresh before access_token expiration
- If refresh_token expires → re-authenticate
- For scheduled tasks: use lifetime B token
```

## Consequences

### Positive

1. **Smooth user experience:** One-step configuration (server URL)
2. **Automatic discovery:** .well-known provides all necessary config
3. **Security:** PKCE for interactive flows, separate tokens for different uses
4. **Flexibility:** Supports deployments with/without client_background
5. **Native SSO:** Same account for all components
6. **Background tasks:** Long-lived tokens for non-interactive actions

### Negative

1. **Dependency on .well-known:** Server must support this specific endpoint
2. **Complexity:** Managing two potential token flows
3. **Secure storage:** Requires secure token storage (keyring/keystore)
4. **Webview:** Need a webview for OIDC flow

### Risks

1. **Token rotation:** Possible loss if refresh_token expires during scheduled task
2. **Compatibility:** Older servers without custom .well-known support
3. **Webview security:** Session hijacking risks in webview

## Migration

**For existing installations:**

- Keep existing tokens until expiration
- New configuration via full flow

**Compatibility version:**

- Minimum server: Twake Server with `/.well-known/twake/desktop-configuration` support

## References

- [OAuth 2.0 RFC 6749](https://tools.ietf.org/html/rfc6749)
- [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html)
- [PKCE RFC 7636](https://tools.ietf.org/html/rfc7636)
- [Twake Well-known Spec](https://docs.twake.com/sso/well-known)
