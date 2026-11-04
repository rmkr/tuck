# Domain documentation

Tuck uses one domain glossary and one directory for architecture decision records, or ADRs. An ADR explains a lasting technical decision and its tradeoffs.

## Before exploring the codebase

Read `GLOSSARY.md` if it exists. Read relevant records in `docs/adr/` if that directory exists. Continue normally when these files have not yet been created.

## Create documents when needed

Use the `domain-modeling` skill to create or update `GLOSSARY.md` when a domain term is resolved. Keep definitions separate from implementation details.

Create an ADR in `docs/adr/` when a consequential design decision needs its reasons recorded. Create these documents as decisions arise.

## Use existing decisions

Use the glossary's terms in code, tests, specs, and tickets. Raise conflicts with an existing ADR before changing the recorded decision.
