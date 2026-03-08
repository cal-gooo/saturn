<script>
  const sections = [
    { id: "overview", label: "Overview" },
    { id: "quick-start", label: "Quick start" },
    { id: "request-model", label: "Request model" },
    { id: "checkout-flow", label: "Checkout flow" },
    { id: "integrations", label: "Integrations" },
    { id: "repo-map", label: "Repository map" }
  ];

  const resources = [
    [
      "Protocol specification",
      "https://github.com/cal-gooo/saturn/blob/main/docs/protocol-spec.md"
    ],
    [
      "Architecture overview",
      "https://github.com/cal-gooo/saturn/blob/main/docs/architecture.md"
    ],
    [
      "Nostr events",
      "https://github.com/cal-gooo/saturn/blob/main/docs/nostr-events.md"
    ],
    ["README", "https://github.com/cal-gooo/saturn/blob/main/README.md"]
  ];

  const repoAreas = [
    ["App config and router", "src/app"],
    ["HTTP handlers and schemas", "src/api"],
    ["Service layer", "src/services"],
    ["Bitcoin settlement adapters", "src/payments"],
    ["Nostr publisher", "src/nostr"],
    ["Joinstr sidecar", "src/privacy"],
    ["Persistence", "src/persistence"],
    ["Protocol docs", "docs"]
  ];
</script>

<div class="site-shell docs-shell docs-shell-gitbook">
  <header class="topbar docs-topbar docs-topbar-gitbook">
    <a class="brand docs-brand" href="/">
      <span class="brand-orb"></span>
      <span>Saturn Docs</span>
    </a>

    <div class="docs-topbar-actions">
      <label class="docs-search" aria-label="Search docs">
        <span>Search docs</span>
        <input type="text" placeholder="Protocol, rails, Nostr..." readonly />
      </label>
      <a class="docs-topbar-link" href="https://github.com/cal-gooo/saturn">GitHub</a>
    </div>
  </header>

  <main class="docs-gitbook-layout">
    <aside class="docs-gitbook-sidebar">
      <div class="docs-sidebar-group">
        <span class="docs-sidebar-label">Getting started</span>
        {#each sections.slice(0, 2) as section}
          <a href={`#${section.id}`}>{section.label}</a>
        {/each}
      </div>

      <div class="docs-sidebar-group">
        <span class="docs-sidebar-label">Core concepts</span>
        {#each sections.slice(2, 5) as section}
          <a href={`#${section.id}`}>{section.label}</a>
        {/each}
      </div>

      <div class="docs-sidebar-group">
        <span class="docs-sidebar-label">Codebase</span>
        <a href="#repo-map">Repository map</a>
      </div>

      <div class="docs-sidebar-group docs-sidebar-group-links">
        <span class="docs-sidebar-label">Source material</span>
        {#each resources as [label, href]}
          <a href={href}>{label}</a>
        {/each}
      </div>
    </aside>

    <section class="docs-gitbook-content">
      <div class="docs-breadcrumbs">
        <a href="/">Saturn</a>
        <span>/</span>
        <span>Documentation</span>
      </div>

      <section id="overview" class="docs-block">
        <p class="docs-kicker">Documentation</p>
        <h1>Saturn documentation</h1>
        <p class="docs-lead">
          Saturn is a seller-side Bitcoin commerce server. It exposes a signed HTTP API for quote,
          checkout, and payment confirmation, uses LDK-backed rails for settlement, and publishes
          capabilities and receipts to Nostr. This page is the fast orientation layer before you
          drop into the full protocol and repository docs.
        </p>

        <div class="docs-note">
          <strong>Current shape</strong>
          <p>
            The implementation is opinionated: BTC-only settlement, Lightning first, optional
            on-chain fallback, deterministic state transitions, and optional Joinstr treasury
            privacy after confirmed settlement.
          </p>
        </div>
      </section>

      <section id="quick-start" class="docs-block">
        <div class="docs-block-header">
          <div>
            <p class="docs-kicker">Quick start</p>
            <h2>Run Saturn locally</h2>
          </div>
          <a class="docs-inline-link" href="https://github.com/cal-gooo/saturn/blob/main/README.md">
            Open README
          </a>
        </div>

        <ol class="docs-ordered">
          <li>Copy the environment file and start Postgres.</li>
          <li>Run the Rust server.</li>
          <li>Run the Svelte website workspace.</li>
          <li>Exercise the signed quote and checkout flow from the README.</li>
        </ol>

        <pre class="docs-code"><code>cp .env.example .env
docker compose up -d postgres
cargo run --bin saturn-server
npm install
npm run website:dev</code></pre>
      </section>

      <section id="request-model" class="docs-block">
        <p class="docs-kicker">Request model</p>
        <h2>Signed envelopes instead of sessions</h2>
        <p>
          Mutating Saturn requests are signed JSON bodies. Each request includes a `message_id`,
          `timestamp`, `nonce`, `public_key`, and `signature`. The body is canonicalized, hashed,
          and verified with secp256k1 before the service layer runs.
        </p>

        <pre class="docs-code"><code>{`{
  "message_id": "uuid-v4",
  "timestamp": "2026-03-08T15:00:00Z",
  "nonce": "opaque-unique-string",
  "public_key": "hex-secp256k1-pubkey",
  "signature": "hex-compact-signature"
}`}</code></pre>

        <ul class="docs-list">
          <li>Replay protection is enforced with timestamp drift and nonce storage.</li>
          <li>Idempotency keys are required for checkout and payment confirmation.</li>
          <li>The request signature is body-centric, not tied to cookies or bearer auth.</li>
        </ul>
      </section>

      <section id="checkout-flow" class="docs-block">
        <p class="docs-kicker">Checkout flow</p>
        <h2>How a Saturn order moves</h2>

        <div class="docs-card-grid">
          <article>
            <strong>Capabilities</strong>
            <p>Expose merchant identity, relay list, settlement rails, and timing windows.</p>
          </article>
          <article>
            <strong>Quote</strong>
            <p>Create a sat-denominated quote and move the order shell into `quoted`.</p>
          </article>
          <article>
            <strong>Checkout intent</strong>
            <p>Generate the Lightning invoice and optionally an on-chain fallback address.</p>
          </article>
          <article>
            <strong>Payment confirm</strong>
            <p>Verify proof, store a receipt, publish a Nostr receipt event, and mark `paid`.</p>
          </article>
        </div>

        <pre class="docs-code"><code>GET  /capabilities
POST /quote
POST /checkout-intent
POST /payment/confirm
GET  /order/:id</code></pre>
      </section>

      <section id="integrations" class="docs-block">
        <p class="docs-kicker">Integrations</p>
        <h2>Rails, relays, and treasury sidecars</h2>

        <h3>Settlement rails</h3>
        <p>
          Saturn uses `ldk-node` for live Lightning and on-chain backends. Lightning handles the
          fast payment path, while on-chain fallback covers harder settlement cases.
        </p>

        <h3>Nostr</h3>
        <p>
          Nostr is used for merchant capability publication, quote references, and receipts. It is
          not the transactional database and it does not replace payment verification.
        </p>

        <h3>Joinstr</h3>
        <p>
          Joinstr is integrated as an optional sidecar for post-settlement treasury privacy. It is
          intentionally kept out of the buyer checkout path.
        </p>

        <pre class="docs-code"><code>APP__LIGHTNING_BACKEND=ldk
APP__ONCHAIN_BACKEND=ldk
APP__COINJOIN_BACKEND=joinstr_sidecar
APP__JOINSTR_SIDECAR_URL=http://127.0.0.1:3011/api/v1/coinjoin/outputs</code></pre>
      </section>

      <section id="repo-map" class="docs-block">
        <p class="docs-kicker">Repository map</p>
        <h2>Where to read next</h2>

        <div class="docs-table">
          {#each repoAreas as [label, path]}
            <div class="docs-table-row">
              <span>{label}</span>
              <code>{path}</code>
            </div>
          {/each}
        </div>
      </section>
    </section>

    <aside class="docs-gitbook-toc">
      <span class="docs-sidebar-label">On this page</span>
      {#each sections as section}
        <a href={`#${section.id}`}>{section.label}</a>
      {/each}
    </aside>
  </main>
</div>
