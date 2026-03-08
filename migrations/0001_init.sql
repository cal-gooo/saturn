CREATE TABLE IF NOT EXISTS quotes (
    id UUID PRIMARY KEY,
    order_id UUID NOT NULL UNIQUE,
    buyer_pubkey TEXT NOT NULL,
    seller_pubkey TEXT NOT NULL,
    quote_payload JSONB NOT NULL,
    total_sats BIGINT NOT NULL,
    status TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    quote_lock_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS orders (
    id UUID PRIMARY KEY,
    quote_id UUID NOT NULL UNIQUE REFERENCES quotes(id) ON DELETE CASCADE,
    buyer_pubkey TEXT NOT NULL,
    seller_pubkey TEXT NOT NULL,
    state TEXT NOT NULL,
    selected_rail TEXT,
    checkout_idempotency_key TEXT UNIQUE,
    payment_confirm_idempotency_key TEXT UNIQUE,
    lightning_invoice TEXT,
    lightning_payment_hash TEXT,
    onchain_address TEXT,
    payment_amount_sats BIGINT,
    settlement_proof JSONB,
    onchain_confirmations INTEGER,
    last_error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS nonces (
    public_key TEXT NOT NULL,
    nonce TEXT NOT NULL,
    message_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (public_key, nonce)
);

CREATE TABLE IF NOT EXISTS receipts (
    id UUID PRIMARY KEY,
    order_id UUID NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    rail TEXT NOT NULL,
    receipt_hash TEXT NOT NULL UNIQUE,
    nostr_event_id TEXT,
    receipt_payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_orders_state ON orders(state);
CREATE INDEX IF NOT EXISTS idx_quotes_expires_at ON quotes(expires_at);
CREATE INDEX IF NOT EXISTS idx_nonces_expires_at ON nonces(expires_at);
