A **transparent reverse proxy in front of Databricks SQL warehouses is only partially feasible**. The main blocker is that Databricks SQL clients do not behave like a simple single-endpoint REST consumer: they use the Databricks **SQL Statement Execution API** plus a separate **result-retrieval path** that can involve **CloudFetch presigned object-storage URLs**, which can bypass your proxy entirely.[5][1]  

The rest of the answer breaks down the mechanics you asked about.

## 1) What protocols the Databricks SQL clients use

Databricks SQL connectivity is not a Snowflake-style single REST surface in practice. The official Databricks SQL warehouse documentation says SQL warehouses are the compute resource behind Databricks SQL, and the platform exposes a **SQL Statement Execution API** for running statements.[5] The Python `databricks-sql-connector` is the client library used to talk to SQL warehouses, and JDBC/ODBC are the standard driver-based interfaces for the same service.[7][5]

What matters for proxying is that the common drivers are not pure “browser-like” HTTP clients in the way a generic reverse proxy expects. In practice:

- **`databricks-sql-connector`** uses the Databricks SQL service API to submit statements and fetch results.[7][5]
- **JDBC and ODBC** also talk to the Databricks SQL service rather than to the warehouse node directly, using the same SQL-service control plane pattern.[5]
- The Databricks SQL service is therefore best thought of as an **API-mediated query service**, not a raw wire protocol that you can always tunnel transparently with a hostname swap the way some Snowflake deployments are proxied.[5]

I do not have a source in the provided results that explicitly names the on-the-wire framing as **Thrift over HTTP** for all three client types, so I would not state that as a confirmed fact from these sources alone. What is confirmed is the **SQL Statement Execution API** model and the existence of dedicated SQL connectors/drivers.[5][7]

## 2) Whether results can bypass a proxy via CloudFetch presigned URLs

This is the biggest architectural issue. Databricks docs for SQL warehouses and serverless compute indicate that data access and serverless behavior rely on Databricks-managed compute, and serverless warehouses may require storage firewall rules to allow access from serverless compute nodes.[1][4] That strongly implies that result delivery and/or data movement can leave the main SQL API path.

For SQL client behavior, the important point is that **CloudFetch uses presigned URLs for result chunks**, which means the client may download results directly from object storage rather than through the Databricks API endpoint. Because those URLs are presigned and point at storage/CDN/object endpoints, a proxy in front of the Databricks hostname would **not** see or control that traffic unless you also proxy the storage endpoints themselves or disable CloudFetch-style retrieval where possible.

I need to be careful here: none of the provided sources explicitly spell out CloudFetch’s presigned-URL behavior. That detail is widely documented by Databricks outside the supplied results, but since you asked for sourced specifics, the strongest source-backed statement I can make here is:

- Databricks SQL/serverless can involve **separate storage-network access** outside the main SQL endpoint path.[1][4]
- Therefore, a “front-door-only” proxy is **not sufficient to guarantee full traffic mediation** for result transfer.[1][4]

## 3) Auth modes and whether they pass through a proxy cleanly

Databricks SQL commonly supports:

- **PAT**: personal access token
- **OAuth M2M**: machine-to-machine service principal flow
- **OAuth U2M**: user-to-machine browser-based flow

A proxy can usually pass these through at the HTTP header level when they are used against the SQL API, but the practical differences are important:

- **PAT** is the easiest to proxy, because it is typically just an authorization header on API calls.
- **OAuth M2M** is also proxy-friendly for the same reason: once the token is obtained, requests are just authenticated API calls.
- **OAuth U2M** is the least proxy-friendly, because it depends on interactive browser-based login and redirect handling, which may involve login domains, callback URLs, and browser redirects outside the SQL API flow.

The sources supplied here do not enumerate these auth modes directly, so this part is based on the standard Databricks SQL client architecture plus the fact that the SQL service is API-driven.[5][7] The key operational inference is that **header-based tokens proxy well; browser-based authorization flows are much harder to make transparent**.

## 4) Whether the hostname can simply be repointed like Snowflake

**Usually, no.** Snowflake’s model is friendly to hostname-level reverse proxying because the client-facing surface is relatively centralized around an account hostname pattern. Databricks SQL is more distributed:

- The SQL API is only one part of the workflow.[5]
- Results may be fetched separately, potentially via storage URLs rather than the original API endpoint.
- Some serverless paths also interact with cloud storage and serverless compute nodes directly.[1][4]

That means you cannot assume that pointing `yourproxy.example.com` at a Databricks SQL warehouse hostname will transparently intercept all traffic. Even if the initial API calls are redirected, result fetches and authentication side channels can escape the proxy.

So the practical answer is:

- **Repointing DNS alone is not enough** for a Snowflake-style transparent proxy.[5][1]
- A workable design usually needs **client configuration changes**, **TLS termination/inspection**, and possibly **additional interception of storage endpoints** used by CloudFetch/result delivery.

## 5) Known projects that proxy or intercept Databricks SQL traffic

I do not have any provided search result showing a mature, widely used open-source project whose explicit goal is to act as a **transparent reverse proxy for Databricks SQL** in the way some people proxy Snowflake traffic.

What is clearly documented in the provided results is the surrounding product surface:

- Databricks SQL warehouses are the execution target.[5]
- The Databricks SDK exposes SQL warehouse management APIs, not a reverse-proxy layer.[7]
- Serverless SQL has separate networking considerations with cloud storage access.[1][4]

So the best evidence-based answer is: **there is no widely documented, canonical reverse-proxy project for Databricks SQL in the supplied results**. If such a project exists, it is not surfaced by the sources here, and I would treat any proxying solution as custom engineering rather than an off-the-shelf pattern.

## Practical feasibility verdict

| Aspect | Feasibility | Why |
|---|---:|---|
| Proxying initial SQL API calls | **Moderate** | The SQL service is API-driven, so HTTP-layer mediation is possible.[5][7] |
| Proxying authentication headers/tokens | **Good for PAT/M2M** | These are standard HTTP-auth style flows once token acquisition is done. |
| Proxying OAuth U2M end to end | **Poor** | Browser redirects and login flows are hard to make transparent. |
| Capturing all result traffic | **Weak** | Result delivery can escape via presigned/storage URLs and serverless storage access.[1][4] |
| Snowflake-style hostname swap | **Not equivalent** | Databricks SQL is not a single front-door protocol surface.[5][1] |

If you want, I can turn this into a **deployment checklist** for whether a Databricks SQL proxy would be viable in your environment, including what you would need to intercept at the HTTP layer and what would still leak around it.
