# Send Checklist (before any outreach is sent)

Complete every item before sending the kickoff/outreach message. **The project will not send anything
until all are checked and explicit send approval is granted.** **No email has been sent.** **NOT
audited / production-ready / mainnet-ready.**

## Pre-send checklist

- [ ] **Branch/head verified** — `testnet/poawx-phase20-blueprint-completion-local`, HEAD `6c7681a`
      (or current), `origin/main` unchanged `19c496dc5f2fa08981a109b10eeb257105c28c43`.
- [ ] **Docs linked** — the message references the correct paths: `SEND_READY_SUMMARY.md`,
      `PACKAGE_MANIFEST.md`, `phase26h-kickoff/` (README → scope → guide → repro).
- [ ] **No secrets included** — no sudo passwords, private keys, wallet data, machine credentials, or
      raw private logs anywhere in the message or attachments. Logs are summarized only.
- [ ] **Placeholders filled** — `[Auditor Name]`, `[Auditor Company]`, `[Contact Email]`, `[Timeline]`,
      `[Budget/Scope Notes]`, `[Your Name / Irium Labs]` all replaced in
      `phase26j-external-handoff/AUDITOR_OUTREACH_MESSAGE.md`.
- [ ] **Recipient confirmed** — the email/contact is verified and correct (right person, right address).
- [ ] **NDA decision made** — NDA sent/signed if required, or explicitly deemed unnecessary (public
      repo, no secrets).
- [ ] **Send approval received** — explicit, recorded approval from the owner to send to the named
      recipient.
- [ ] **Copy saved to audit folder** — the exact sent message is archived (e.g.
      `docs/audit/phase26l-engagement-tracker/sent/SENT_<date>_<auditor>.md`) for traceability. Strip
      any private contact details not appropriate for the repo, or keep the archive outside the repo if
      it would contain personal data.
- [ ] **Follow-up date set** — a reminder date to follow up if no response.

## After sending

- [ ] Update `AUDIT_ENGAGEMENT_STATUS.md`: Current status → **Contacted**; set the date.
- [ ] Update `NEXT_STEPS_TRACKER.md` steps 1–5 → Done; advance 6–9 to In progress.
- [ ] Record the recipient + date in the engagement tracker (not the contents of any secret).

## Guardrails

- Do not auto-send. A human must perform the actual send after this checklist is fully satisfied.
- Do not invent or guess a recipient. If any item is unchecked, **do not send**.
- Sending the package does not change any claim: still not-audited / not-production-ready /
  not-mainnet-ready. Public testnet and mainnet remain gated/blocked.
