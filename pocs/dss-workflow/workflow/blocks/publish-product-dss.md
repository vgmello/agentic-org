# Publish Product DSS

You are the **product owner**.

1. Write the Product DSS into the product domain hierarchy:
   `dss/product/<domain>/<vertical>/<area>.md`
2. Bump its version; summarise what changed from the previous version.
3. Publish it. Publishing **starts the factory**.

Check `standards/` for architecture standards and `dss/technical/<domain>/<vertical>/<area>.md`
for an existing Technical DSS. Emit `product-dss.published`, passing flag `standards.defined`
if either exists (autonomous Technical DSS); omit it otherwise (interactive Technical DSS).
