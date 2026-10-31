---
id: gotcha_reflexion_4625153e
kind: gotcha
topic_key: gotcha_prevention
title: "[REFLEXION GOTCHA] Caller and callee signatures must maintain arity parity."
author: ozybrain_reflexion
date: "2026-10-31"
severity: high
applies_to:
  - "src/*"
tags:
  - reflexion
  - failure_triangulation
  - auto_learned
---

# Verbal Self-Reflection (Reflexion Engine)

- **Flawed Assumption:** Assumed change sequence for 'corregir exportacion excel' would succeed without contract verification.
- **Violated Invariant:** Caller and callee signatures must maintain arity parity.
- **Prescriptive Antidote:** Apply isolated boundary check and verify affected files: target files.

### Error Trace Signature:
```text
TypeError: resolve_export_payload() got an unexpected keyword argument 'model_cls' at app/modules/sucs_aashto/router.py:529 in descargar_excel_post
```
