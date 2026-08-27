# License entitlement design

QuotaFence does not currently issue, store, or verify commercial licenses. This
document fixes the boundary and threat model before a payment implementation is
chosen.

## Goals

- Keep the Free core fully functional without an account or network.
- Let a locally verified grant add capabilities without teaching the quota
  engine about products, prices, or payment providers.
- Support offline use and temporary service outages.
- Fall back to Free without deleting, rewriting, or hiding user data.
- Keep purchase identity and billing records out of the quota database.

## Non-goals

- Perfect DRM on a user-controlled open-source client.
- Device fingerprinting or binding a purchase to hardware in the first paid
  release.
- Sending source code, prompts, transcripts, agent credentials, folder names,
  or quota history to a licensing service.
- Using a payment webhook response directly as a trusted client entitlement.

## Proposed grant envelope

The exact encoding and signature algorithm must be selected during the Pro
implementation. The logical payload should be versioned and resemble:

```json
{
  "schemaVersion": 1,
  "licenseId": "lic_...",
  "issuedAt": 1787800000000,
  "validFrom": 1787800000000,
  "expiresAt": 1790478400000,
  "capabilities": ["advanced_analytics", "smart_alerts"],
  "signature": "base64url-signature"
}
```

The server signs the canonical payload with an offline-protected private key.
The app embeds only the public verification key. The payment provider never
supplies trusted capability names directly to the app.

## Resolution flow

1. Read a cached envelope from a license-specific local store, not the quota
   SQLite database.
2. Reject unknown schema versions, malformed values, unsupported capabilities,
   invalid signatures, and inverted validity windows.
3. Check `validFrom <= now < expiresAt` using explicit milliseconds.
4. Merge verified capabilities on top of the immutable Free capability set.
5. Return a snapshot through the same Tauri command used when offline.
6. On every failure, return Free and preserve all application data.

The current Rust resolver implements steps 3–6 for an already verified grant.
Signature verification and storage must remain adapters feeding that resolver.

## Offline and recovery behavior

- A valid cached signed grant works offline until its signed expiry.
- Network failure alone does not remove capabilities before that expiry.
- The app should warn before expiry and offer purchase recovery, but must not
  interrupt a running agent process solely because entitlement refresh failed.
- Clock rollback/large clock jumps require a documented policy before launch;
  the client must never extend a signed expiry by rewriting local timestamps.
- Reinstall and device replacement use an explicit recovery flow with rate
  limits at the service, not hidden hardware identifiers.
- Cancellation normally remains active until the already-paid period ends.
  Refund and chargeback behavior must be defined in the published terms.

## Key and incident operations

- Never place the signing private key in the repository, desktop app, CI logs,
  or ordinary build secrets available to pull requests.
- Version verification keys so a later app can trust a controlled overlap
  during rotation.
- Maintain a revocation/reissue procedure for a compromised key; do not depend
  on silent replacement of already published binaries.
- Keep updater signing keys separate from entitlement signing keys.
- Test invalid signature, unknown key, future grant, exact expiry boundary,
  corrupted cache, offline launch, reinstall, refund, and service outage.

## Privacy contract for license requests

A license request may contain only the data necessary to recover/refresh a
purchase, such as a license identifier, product version, platform, and a random
installation identifier if justified. It must not contain workspace paths,
project names, quota values, prompts, source code, transcripts, or provider
credentials. Every transmitted field and retention period must appear in the
published privacy policy before the endpoint is enabled.

