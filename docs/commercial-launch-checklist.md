# Commercial launch checklist

This is the release gate for offering QuotaFence to general users or accepting
payment. It is an engineering and operations checklist, not legal advice.

## Product correctness

- [ ] Complete the dated installed-app matrix in `beta-exit-checklist.md` with
  external testers on clean accounts.
- [ ] Close correctness and false-block failures for Codex and Claude Code.
- [ ] Verify reset rollover, provider corrections, ambiguous attribution,
  disable/uninstall, and recovery in packaged builds.
- [ ] Make provider precision and enforcement limits visible at the decision
  point.
- [ ] Verify a license outage or expiry leaves Free features and user data
  available before any license system ships.

## Distribution and platform trust

- [ ] Enroll in the Apple Developer Program; configure Developer ID signing and
  notarization in the release workflow.
- [ ] Run Gatekeeper install, upgrade, and uninstall tests on a clean macOS
  account.
- [ ] Complete the native Windows smoke matrix, including console lifecycle and
  recovery.
- [ ] Select and secure a Windows code-signing certificate; test SmartScreen
  behavior on a clean account.
- [ ] Publish immutable checksums, build provenance, release notes, and a tested
  rollback procedure for both platforms.
- [ ] Design updater signing/key rotation before enabling automatic updates.

Official signed binaries should be available to Free users. Security fixes must
not depend on a paid subscription.

## Privacy, security, and support

- [ ] Publish a privacy policy naming the operator, contact address, data
  categories, purposes, retention, subprocessors, user rights, and deletion
  path.
- [ ] Publish terms, refund/cancellation terms, and a support policy reviewed
  for the jurisdictions where sales are offered.
- [ ] Document exactly what remains local and every optional network request.
- [ ] Add a security reporting address and vulnerability response process.
- [ ] Enable and test GitHub private vulnerability reporting before changing
  repository visibility; the API did not expose that channel while this
  checklist was written.
- [ ] Audit repository history, CI logs, artifacts, and application bundles for
  credentials and signing secrets before making the repository public.
- [ ] Test backup/export and confirm uninstall does not silently destroy user
  data.

Do not copy a generic privacy or terms template and present it as reviewed.
Operator identity, jurisdiction, tax handling, and support promises require
real business decisions.

## Sales and entitlement operations — only after a Pro workflow exists

- [ ] Choose a Merchant of Record or document direct tax/VAT responsibilities.
- [ ] Implement checkout, webhook idempotency, signed grants, local cached
  verification, offline grace behavior, recovery, cancellation, and refunds.
- [ ] Keep payment customer data out of the quota database.
- [ ] Keep entitlement checks centralized and capability-based.
- [ ] Test downgrade, clock skew, corrupted cache, server outage, chargeback,
  refund, and reinstall cases without data loss.
- [ ] Provide a self-service way to recover a purchase and manage billing.

## Launch evidence

- [ ] Public release exists for macOS and supported Windows targets.
- [ ] Download/install path is tested by someone outside the development
  machine.
- [ ] Pricing page matches the actual capability registry and refund terms.
- [ ] Free remains genuinely useful with unlimited projects.
- [ ] At least one Pro workflow is end-to-end useful without cloud access unless
  its purpose is explicitly sync or remote delivery.
- [ ] Support channel, issue triage owner, crash-response owner, and release
  owner are named.
