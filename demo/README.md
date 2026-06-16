# chukei demo corpus

`query-history.csv` is a small, synthetic Snowflake `ACCOUNT_USAGE.QUERY_HISTORY`
export used for release evidence and quick local replay checks. It contains no
customer data and no credentials.

Generate the same style of signed demo evidence attached to GitHub releases:

```bash
scripts/release/build-demo-assets.sh v0.2.2
```

Verify an evidence envelope:

```bash
chukei evidence verify --file target/release-demo-assets/chukei-v0.2.2-demo-projection.evidence.json
```
