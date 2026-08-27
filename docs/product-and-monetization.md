# Product and monetization direction

QuotaFence is a local-first control plane for AI coding agents. Its primary job
is not merely to display usage: it observes provider capacity, allocates it to
folder-based projects, and enforces local policy before work consumes a scarce
allowance.

## Product promise

> Observe → allocate → enforce → forecast → automate → coordinate.

The open core owns the first four steps at a useful basic level. Paid features
sell convenience, deeper analysis, automation, and coordination; they do not
turn project quotas into a demo or require source code, prompts, transcripts,
or credentials to leave the device.

## Free and open-source core

The Apache-2.0 core includes:

- local dashboards and provider usage collection;
- multi-agent provider adapters;
- folder detection, unlimited projects, and manual configuration;
- project quota allocation plus local Warn/Stop enforcement;
- basic history, alerts, forecast, and data export; and
- the ability to build the desktop app and CLI from source.

Official release artifacts should also be signed when production credentials
are available. Signing is a software-supply-chain safety property, not a paid
feature. Free users must continue to receive important security fixes even if
automatic background updating is sold as convenience.

## QuotaFence Pro

The initial price hypothesis is USD 3/month or USD 29/year, with optional
grandfathering for early adopters. Pricing is not a code contract and must be
validated before launch.

Candidate Pro capabilities are:

- advanced analytics, longer history, forecasts, and scenario planning;
- smart alerts, advanced system notifications, and scheduled reports;
- advanced CSV/JSON exports;
- rules by time, agent, model, and project;
- automatic agent/model routing near quota boundaries;
- optional encrypted multi-device configuration sync and backup/restore;
- automatic update convenience; and
- priority support.

The first paid release should ship at least one complete, clearly useful Pro
workflow before accepting payment. A disabled menu or license check by itself
is not a sellable feature.

## Team, later

Team is not part of the MVP. Demand may eventually justify shared budgets,
policy templates, member limits, audit logs, centralized reports, managed
configuration, roles, and SSO. Pricing should wait for direct customer
evidence.

## Entitlement architecture

The Rust entitlement module is the source of truth. Product code asks for
capabilities, never a plan name:

```text
UI / CLI
   │ asks "is capability X available?"
   ▼
Entitlement snapshot
   ├── always includes Free core capabilities
   └── may add capabilities from a locally verified, unexpired grant
          │
          └── payment/license adapter (future; outside quota engine)
```

Rules:

- Do not scatter `isPro`, price, or billing-provider checks through the app.
- The quota domain and local database must not depend on a cloud service.
- A missing, invalid, offline, or expired license falls back to Free.
- Expiry never deletes data or prevents basic export.
- Sync is opt-in; no prompt, source code, transcript, or credential is sent.
- A payment provider webhook may issue an entitlement, but payment state is not
  itself a quota-domain concept.
- Keep proprietary Pro implementations outside Apache-2.0 core modules if they
  are not intended to inherit the repository license.

The current implementation exposes the Free snapshot through Tauri IPC and has
a provider-neutral resolver for future verified grants. It intentionally does
not yet store a license, call a license server, or implement payment.

## Launch order

1. Pass the external installed-app and native Windows beta gates.
2. Publish trustworthy signed artifacts and clear privacy/support policies.
3. Measure activation, sync accuracy, false blocks, and retention with
   privacy-preserving opt-in feedback rather than source/prompt telemetry.
4. Build one complete Pro workflow, most likely advanced forecast plus smart
   alerts.
5. Add a Merchant-of-Record checkout, signed license grants, recovery, and
   self-service cancellation only when the paid workflow is ready.
6. Add optional sync and Team only after observed demand.

