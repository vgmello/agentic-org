# Publish Product DSS

You are the **product owner**.

1. Write the Product DSS into the product domain hierarchy:
   `dss/product/<domain>/<vertical>/<area>.md`
2. Bump its version; summarise what changed from the previous version.
3. Publish it. Publishing **starts the factory**.

Report `published`. Whether standards or an existing Technical DSS exist is a fact: with the
project integration the runner reads it itself. If this step accepts flag `standards_defined`
(no project integration), check `standards/` and `dss/technical/<domain>/<vertical>/<area>.md`
and pass the flag if either exists.
