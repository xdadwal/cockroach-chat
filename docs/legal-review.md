# Pre-release notices review

Reviewed on 2026-10-10 for the proposed v0.1.0 release. This is an engineering review of repository
wording and packaged notices, not a legal opinion or certification that publication or use is lawful.

## Findings and changes

| Area | Finding | Change |
|---|---|---|
| Project license | The MIT license already contains a warranty disclaimer and limitation of liability. | Retained its text unchanged and added a verbatim copy inside the APK. |
| Intended purpose | The README promoted protest/disaster use and contained broad security and delivery claims. | Reframed active documentation, onboarding, Credits, and release notes around education, research, and controlled experimentation; removed or qualified those claims. |
| Independence | Credits and third-party references did not explain whether endorsement was implied. | Added explicit non-endorsement and independence wording in `DISCLAIMER.md`, README, notices, and app Credits. |
| Historical material | Research and planning files contain deployment scenarios and comparisons. | Marked them as historical material rather than current advice or assurances; preserved the record. |
| Binary notices | Only font license texts were explicitly included in the APK assets. | Added the project license, disclaimer, and a dependency notice inventory. |
| Dependency inventory | Source-package licenses and binary notices are separate from an allowlist check. | Collected texts from 145 external Cargo packages and the 62 distinct Android release dependency coordinates, including vendored SQLCipher/OpenSSL/libffi texts and MPL source locations. |

The Rust inventory includes build-only and non-Android packages; it is deliberately broader than
the shipped native library. Android license metadata was checked against the exact-version POMs
from Google Maven and Maven Central, including inherited parent metadata. Embedded archive notices
were retained. JNA is used under its Apache-2.0 option. The font licenses remain separate.

## What the wording does and does not mean

“Solely for education, research, and controlled experimentation” describes the maintainers'
purpose. It is not a new restriction on recipients. MIT continues to permit other uses, including
commercial use, under its existing terms. Adding an enforceable educational-only condition would
require a separate licensing decision; it would not be the standard MIT license or meet the Open
Source Definition's field-of-endeavor criterion.

The notice avoids promising safety, delivery, anonymity, availability, forensic erasure, or legal
compliance. It states that references and third-party use do not imply the project's endorsement.
These statements do not prevent someone from bringing a claim or establish the enforceability of
a disclaimer in a particular jurisdiction.

## Scope limits

- No trademark or name clearance was performed for “Cockroach Chat,” its logo, or other branding.
  A non-affiliation sentence does not establish a right to use a name or mark.
- No jurisdiction-specific assessment of liability, consumer law, privacy, radio regulation,
  cryptography/export requirements, or a particular deployment was performed. Educational intent
  is not regulatory approval or an exemption.
- Dependency metadata and packaged notice checks are not a complete intellectual-property audit
  of all source, generated code, images, or contributions. Upstream license changes and future
  dependency changes need review.
- The app's technical security properties have not been independently audited. This review does
  not change that status.

A qualified lawyer should assess jurisdiction-specific protection and any proposed restrictive
license. Do not describe this review as legal clearance or protection against all claims.

## Primary references

- [MIT license](https://opensource.org/license/mit): permissions, notice condition, warranty, and liability text.
- [Open Source Definition, section 6](https://opensource.org/osd): field-of-endeavor restrictions.
- [Apache License 2.0, sections 4 and 6](https://www.apache.org/licenses/LICENSE-2.0): redistribution notices and trademarks.
- [MPL 2.0, sections 3.1–3.4](https://www.mozilla.org/en-US/MPL/2.0/): covered-source availability and notice obligations.
