<script>
  import heroArt from "../../../assets/saturn-hero.svg";
  const repoUrl = "https://github.com/cal-gooo/saturn";
  const donationAddresses = [
    {
      label: "Bitcoin",
      value: "bc1qf5f04hsx72k88hq9kz0ycat45eg5c2uxl4lwsf"
    },
    {
      label: "Lightning",
      value: "quickzebra13@primal.net"
    }
  ];
  let copiedValue = "";

  async function copyDonationValue(value) {
    try {
      await navigator.clipboard.writeText(value);
      copiedValue = value;
      setTimeout(() => {
        if (copiedValue === value) {
          copiedValue = "";
        }
      }, 1800);
    } catch {
      copiedValue = "";
    }
  }

  const featureGroups = [
    {
      eyebrow: "BTC rails",
      title: "Lightning first. On-chain when it matters.",
      description:
        "Saturn creates Lightning invoices for the fast path, then falls back to Bitcoin addresses when settlement needs a harder rail.",
      bullets: [
        "Real `ldk-node` adapters for Lightning and on-chain flows",
        "Quote-time rail negotiation with deterministic checkout behavior",
        "Live regtest coverage for router-level payment confirmation"
      ]
    },
    {
      eyebrow: "Signed commerce",
      title: "No sessions. No bearer tokens. Just signed intent.",
      description:
        "Every mutating request is a signed JSON envelope with canonical hashing, timestamp checks, replay protection, and idempotency.",
      bullets: [
        "Body-level secp256k1 signatures",
        "Nonce replay protection and message correlation",
        "Strict order transitions from quote to paid state"
      ]
    },
    {
      eyebrow: "Nostr-native",
      title: "Identity and receipts live in the open.",
      description:
        "Saturn publishes merchant capabilities, quote references, and receipts to Nostr relays without making Nostr the payment transport itself.",
      bullets: [
        "Merchant capability publication",
        "Quote and receipt event references",
        "Relay redundancy around public attestations"
      ]
    },
    {
      eyebrow: "Treasury privacy",
      title: "Joinstr adds privacy where merchant treasury needs it.",
      description:
        "Confirmed on-chain outputs can be handed to a Joinstr sidecar after settlement, helping merchants improve UTXO privacy without pushing CoinJoin complexity into the buyer checkout flow.",
      bullets: [
        "Optional sidecar integration",
        "Best-effort enqueue after confirmed on-chain settlement",
        "No impact on checkout success if the sidecar is unavailable"
      ]
    }
  ];

  const stack = [
    "Rust + Axum core",
    "Svelte + Vite official website",
    "Postgres for durable order state",
    "Nostr SDK for relay publishing",
    "LDK for Lightning and Bitcoin rails",
    "Joinstr sidecar for post-settlement privacy"
  ];

  const agentEconomyReasons = [
    {
      title: "Machine-native payments",
      text: "Agents need a payment rail they can invoke, verify, and settle without card networks, invoicing portals, or human approval loops."
    },
    {
      title: "Deterministic trade flow",
      text: "Quotes, checkout, and payment confirmation follow an auditable state machine, which makes automated buying and selling safer to coordinate."
    },
    {
      title: "Open Bitcoin rails",
      text: "Bitcoin and Lightning give agents an internet-native settlement path that works across apps, platforms, and counterparties without platform lock-in."
    }
  ];

  const narrative = [
    {
      label: "1. Discover",
      text: "Buyer agents fetch Saturn capabilities and learn which rails, relays, and settlement windows the merchant is willing to support."
    },
    {
      label: "2. Quote",
      text: "A signed quote request locks the commerce payload, turns line items into sats, and emits a deterministic quoted order."
    },
    {
      label: "3. Checkout",
      text: "The buyer chooses a rail, Saturn generates a Lightning invoice, and an on-chain fallback address can be provisioned in the same flow."
    },
    {
      label: "4. Confirm",
      text: "Settlement proofs are verified against the selected backend, receipts are published, and the order becomes paid."
    }
  ];
</script>

<svelte:head>
  <meta property="og:title" content="Saturn | BTC-native Agent Commerce" />
  <meta
    property="og:description"
    content="Artistic landing page for Saturn, the Bitcoin-only seller stack for AI agent commerce."
  />
</svelte:head>

<div class="site-shell">
  <div class="cosmos-bg cosmos-bg-a"></div>
  <div class="cosmos-bg cosmos-bg-b"></div>
  <div class="constellation constellation-left"></div>
  <div class="constellation constellation-right"></div>

  <header class="topbar">
    <a class="brand" href="/">
      <span class="brand-orb"></span>
      <span>Saturn</span>
    </a>

    <nav class="nav">
      <a href="#features">Features</a>
      <a href={repoUrl}>GitHub</a>
    </nav>
  </header>

  <main>
    <section class="hero panel">
      <div class="hero-copy">
        <p class="eyebrow">Bitcoin-only AI agent commerce</p>
        <h1>
          The seller stack for
          <span>AI agents</span>
          transacting in Bitcoin.
        </h1>
        <p class="lede">
          Saturn turns signed agent intent into quotes, Lightning invoices, on-chain fallback,
          receipts, and relay-published proof. It is opinionated, Rust-native, and built
          for teams that want AI agents to buy and sell with Bitcoin, not cards, fiat, or
          generic checkout middleware.
        </p>
        <p class="hero-subnote">
          As software agents start procuring services, paying APIs, and settling autonomous work,
          Saturn gives sellers a merchant stack designed for that machine-to-machine economy.
        </p>

        <div class="cta-row">
          <a class="button button-primary" href={repoUrl}>View the repo</a>
        </div>

        <div class="stat-grid">
          <article>
            <strong>Lightning + on-chain</strong>
            <span>One checkout shape, two Bitcoin rails.</span>
          </article>
          <article>
            <strong>Signed envelopes</strong>
            <span>Self-authenticating request bodies with replay protection.</span>
          </article>
          <article>
            <strong>Nostr receipts</strong>
            <span>Relay-published capabilities, quote refs, and payment proof.</span>
          </article>
        </div>
      </div>

      <div class="hero-visual">
        <div class="orbit-card">
          <div class="orbit-halo"></div>
          <img src={heroArt} alt="Stylized Saturn planet banner" />
          <div class="orbit-caption">
            <span>Agent quote locked</span>
            <span>Lightning invoice issued</span>
            <span>Receipt published to relays</span>
          </div>
        </div>
      </div>
    </section>

    <section class="marquee-strip">
      {#each stack as item}
        <span>{item}</span>
      {/each}
    </section>

    <section class="section">
      <div class="section-heading">
        <p class="eyebrow">Support Saturn</p>
        <h2>Donate to help push Bitcoin-native agent commerce forward.</h2>
        <p>
          If Saturn is useful to you, you can support the project directly with Bitcoin or
          Lightning. Both addresses below are one-click copyable.
        </p>
      </div>

      <div class="donation-grid">
        {#each donationAddresses as donation}
          <article class="panel donation-card">
            <p class="eyebrow">{donation.label}</p>
            <div class="donation-address">
              <code>{donation.value}</code>
            </div>
            <button
              class="button button-secondary donation-copy"
              type="button"
              on:click={() => copyDonationValue(donation.value)}
            >
              {copiedValue === donation.value ? "Copied" : "Copy"}
            </button>
          </article>
        {/each}
      </div>
    </section>

    <section id="features" class="section">
      <div class="section-heading">
        <p class="eyebrow">Why Saturn</p>
        <h2>Built for teams that want open rails without chaos.</h2>
        <p>
          Saturn is narrow on purpose. It does fewer things than a mainstream commerce
          platform, but it does them in a way that is legible, composable, and deeply Bitcoin-native.
        </p>
      </div>

      <div class="feature-grid">
        {#each featureGroups as group}
          <article class="feature-card panel">
            <p class="eyebrow">{group.eyebrow}</p>
            <h3>{group.title}</h3>
            <p>{group.description}</p>
            <ul>
              {#each group.bullets as bullet}
                <li>{bullet}</li>
              {/each}
            </ul>
          </article>
        {/each}
      </div>
    </section>

    <section class="section">
      <div class="section-heading">
        <p class="eyebrow">Agent economy</p>
        <h2>Why sellers will need infrastructure built for agents.</h2>
        <p>
          The future agentic economy is not just more checkout pages. It is software buying from
          software, with payment, verification, and receipts handled by systems that can reason and
          act on their own. Saturn is built for that shift.
        </p>
      </div>

      <div class="feature-grid">
        {#each agentEconomyReasons as reason}
          <article class="feature-card panel">
            <h3>{reason.title}</h3>
            <p>{reason.text}</p>
          </article>
        {/each}
      </div>
    </section>

    <section class="section split-layout">
      <div class="panel timeline-panel">
        <p class="eyebrow">Why Bitcoin only</p>
        <h2>Saturn is narrow because agent payments need clearer guarantees.</h2>
        <div class="timeline">
          <article>
            <h3>Programmable settlement</h3>
            <p>
              Bitcoin and Lightning are easier for software to invoke, verify, and reconcile than
              card networks built around human checkout and manual risk controls.
            </p>
          </article>
          <article>
            <h3>Open rails</h3>
            <p>
              Agents need payment rails that work across platforms and counterparties without
              depending on a single marketplace, gateway, or closed wallet provider.
            </p>
          </article>
          <article>
            <h3>Global machine commerce</h3>
            <p>
              Bitcoin gives Saturn one internet-native asset and one consistent settlement model,
              which keeps the protocol simpler for autonomous buyers and sellers.
            </p>
          </article>
        </div>
      </div>

      <div class="panel code-panel">
        <p class="eyebrow">Scope boundary</p>
        <h2>What Saturn is intentionally not trying to be.</h2>
        <div class="code-stack">
          <div>
            <span class="code-label">Not card middleware</span>
            <p>Saturn does not try to wrap cards, fiat wallets, chargebacks, or legacy ecommerce rails.</p>
          </div>
          <div>
            <span class="code-label">Not generic checkout</span>
            <p>It is built for AI agents transacting in Bitcoin, not for every merchant use case on the internet.</p>
          </div>
          <div>
            <span class="code-label">Not multi-rail sprawl</span>
            <p>Staying Bitcoin-only keeps the protocol legible, the backend simpler, and settlement verification coherent.</p>
          </div>
        </div>
      </div>
    </section>

    <section id="flow" class="section split-layout">
      <div class="panel timeline-panel">
        <p class="eyebrow">Current design</p>
        <h2>What the protocol does right now.</h2>
        <div class="timeline">
          {#each narrative as item}
            <article>
              <h3>{item.label}</h3>
              <p>{item.text}</p>
            </article>
          {/each}
        </div>
      </div>

      <div class="panel code-panel">
        <p class="eyebrow">Feature map</p>
        <h2>Everything the current Saturn stack covers.</h2>
        <div class="code-stack">
          <div>
            <span class="code-label">Commerce API</span>
            <p>Capabilities, quote, checkout intent, payment confirm, order lookup.</p>
          </div>
          <div>
            <span class="code-label">State model</span>
            <p>Deterministic transitions from created to paid, with explicit terminal states.</p>
          </div>
          <div>
            <span class="code-label">Settlement</span>
            <p>Lightning-first, on-chain fallback, live LDK-backed verification paths.</p>
          </div>
          <div>
            <span class="code-label">Identity + proof</span>
            <p>Nostr capability publication, quote references, and signed payment receipts.</p>
          </div>
          <div>
            <span class="code-label">Operations</span>
            <p>Postgres persistence, regtest coverage, and optional Joinstr sidecar handoff.</p>
          </div>
        </div>
      </div>
    </section>

    <section class="section">
      <div class="callout panel">
        <div>
          <p class="eyebrow">Read further</p>
          <h2>Docs, protocol notes, and code are one click away.</h2>
          <p>
            The docs page maps the protocol spec, architecture notes, Nostr event model, and
            repository references into a single launchpad.
          </p>
        </div>
        <a class="button button-primary" href={repoUrl}>GitHub</a>
      </div>
    </section>
  </main>
</div>
