import { createHash, randomBytes, randomUUID } from "node:crypto";
import { secp256k1 } from "@noble/curves/secp256k1";

const PLUGIN_ID = "openclaw-saturn-plugin";
const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_SETTLEMENT_PREFERENCE = "lightning_with_onchain_fallback";
const DEFAULT_SELECTED_RAIL = "lightning";

export const id = PLUGIN_ID;

function asObject(value, fieldName) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${fieldName} must be an object`);
  }
  return value;
}

function getPluginConfig(api) {
  const cfg = api?.config?.plugins?.entries?.[PLUGIN_ID]?.config;
  return cfg && typeof cfg === "object" ? cfg : {};
}

function cleanHex(value) {
  return value.startsWith("0x") ? value.slice(2) : value;
}

function parsePrivateKey(secretHex) {
  const normalized = cleanHex(String(secretHex || "")).trim();
  if (!/^[0-9a-fA-F]{64}$/.test(normalized)) {
    throw new Error("requestSigningSecretKey must be 64 hex chars");
  }
  const keyBytes = Uint8Array.from(Buffer.from(normalized, "hex"));
  if (!secp256k1.utils.isValidPrivateKey(keyBytes)) {
    throw new Error("requestSigningSecretKey is not a valid secp256k1 private key");
  }
  return keyBytes;
}

function deriveCompressedPublicKeyHex(privateKeyBytes) {
  return Buffer.from(secp256k1.getPublicKey(privateKeyBytes, true)).toString("hex");
}

function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }

  if (value && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort()) {
      out[key] = canonicalize(value[key]);
    }
    return out;
  }

  return value;
}

function payloadHashWithoutSignature(envelope) {
  const sanitized = { ...envelope };
  delete sanitized.signature;
  const canonicalJson = JSON.stringify(canonicalize(sanitized));
  return createHash("sha256").update(canonicalJson).digest();
}

function signDigestHex(digest, privateKeyBytes) {
  const signature = secp256k1.sign(Uint8Array.from(digest), privateKeyBytes, { lowS: true });
  if (typeof signature.toCompactHex === "function") {
    return signature.toCompactHex();
  }
  return Buffer.from(signature.toCompactRawBytes()).toString("hex");
}

function buildSignedEnvelope(payload, secretHex) {
  const privateKeyBytes = parsePrivateKey(secretHex);
  const envelope = {
    message_id: randomUUID(),
    timestamp: new Date().toISOString(),
    nonce: randomBytes(16).toString("hex"),
    public_key: deriveCompressedPublicKeyHex(privateKeyBytes),
    signature: "",
    ...asObject(payload, "payload")
  };

  const digest = payloadHashWithoutSignature(envelope);
  envelope.signature = signDigestHex(digest, privateKeyBytes);
  return envelope;
}

function textResult(value) {
  return {
    content: [
      {
        type: "text",
        text: typeof value === "string" ? value : JSON.stringify(value, null, 2)
      }
    ]
  };
}

function getRequiredConfig(pluginConfig, key) {
  const value = pluginConfig?.[key];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`Missing plugin config: ${key}`);
  }
  return value.trim();
}

function normalizeBaseUrl(baseUrl) {
  return baseUrl.replace(/\/+$/, "");
}

function requestTimeout(pluginConfig) {
  const value = pluginConfig?.requestTimeoutMs;
  if (!Number.isFinite(value)) {
    return DEFAULT_TIMEOUT_MS;
  }
  return Math.max(1000, Number(value));
}

async function saturnRequest(pluginConfig, method, path, body, extraHeaders = {}) {
  const baseUrl = normalizeBaseUrl(getRequiredConfig(pluginConfig, "saturnBaseUrl"));
  const timeoutMs = requestTimeout(pluginConfig);
  const url = `${baseUrl}${path}`;
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(url, {
      method,
      headers: {
        "content-type": "application/json",
        ...extraHeaders
      },
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal
    });

    const contentType = response.headers.get("content-type") || "";
    const isJson = contentType.includes("application/json");
    const payload = isJson ? await response.json() : await response.text();

    if (!response.ok) {
      throw new Error(`Saturn ${method} ${path} failed (${response.status}): ${typeof payload === "string" ? payload : JSON.stringify(payload)}`);
    }

    return payload;
  } finally {
    clearTimeout(timeout);
  }
}

async function saturnSignedPost(pluginConfig, path, payload, idempotencyKey) {
  const secret = getRequiredConfig(pluginConfig, "requestSigningSecretKey");
  const signedEnvelope = buildSignedEnvelope(payload, secret);
  const headers = idempotencyKey ? { "Idempotency-Key": idempotencyKey } : {};
  return saturnRequest(pluginConfig, "POST", path, signedEnvelope, headers);
}

async function getCapabilities(pluginConfig) {
  return saturnRequest(pluginConfig, "GET", "/capabilities");
}

function asStringArray(values, fieldName) {
  if (!Array.isArray(values) || values.length === 0) {
    throw new Error(`${fieldName} must be a non-empty array`);
  }
  const out = values.map((value) => String(value));
  return out;
}

function asItems(values) {
  if (!Array.isArray(values) || values.length === 0) {
    throw new Error("items must be a non-empty array");
  }
  return values.map((raw, index) => {
    const value = asObject(raw, `items[${index}]`);
    return {
      sku: String(value.sku),
      description: String(value.description),
      quantity: Number(value.quantity),
      unit_price_sats: Number(value.unit_price_sats)
    };
  });
}

function pickSettlementPreference(params, pluginConfig) {
  return (
    params.settlement_preference ||
    pluginConfig.defaultSettlementPreference ||
    DEFAULT_SETTLEMENT_PREFERENCE
  );
}

function pickSelectedRail(params, pluginConfig) {
  return params.selected_rail || pluginConfig.defaultSelectedRail || DEFAULT_SELECTED_RAIL;
}

function parseOptionalString(value) {
  if (value === undefined || value === null) {
    return undefined;
  }
  const v = String(value).trim();
  return v === "" ? undefined : v;
}

export default function registerSaturnTools(api) {
  api.registerTool(
    {
      name: "saturn_get_capabilities",
      description: "Fetch seller capabilities from Saturn.",
      parameters: { type: "object", properties: {}, additionalProperties: false },
      async execute() {
        const pluginConfig = getPluginConfig(api);
        const response = await getCapabilities(pluginConfig);
        return textResult(response);
      }
    },
    { optional: true }
  );

  api.registerTool(
    {
      name: "saturn_create_quote",
      description: "Create a signed quote request in Saturn.",
      parameters: {
        type: "object",
        properties: {
          buyer_nostr_pubkey: { type: "string" },
          seller_nostr_pubkey: { type: "string" },
          callback_relays: { type: "array", items: { type: "string" } },
          items: {
            type: "array",
            items: {
              type: "object",
              properties: {
                sku: { type: "string" },
                description: { type: "string" },
                quantity: { type: "number" },
                unit_price_sats: { type: "number" }
              },
              required: ["sku", "description", "quantity", "unit_price_sats"],
              additionalProperties: false
            }
          },
          settlement_preference: {
            type: "string",
            enum: ["lightning_only", "lightning_with_onchain_fallback"]
          },
          buyer_reference: { type: "string" }
        },
        required: ["buyer_nostr_pubkey", "items"],
        additionalProperties: false
      },
      async execute(_id, params) {
        const pluginConfig = getPluginConfig(api);
        const caps = await getCapabilities(pluginConfig);
        const payload = {
          buyer_nostr_pubkey: String(params.buyer_nostr_pubkey),
          seller_nostr_pubkey:
            parseOptionalString(params.seller_nostr_pubkey) || caps.merchant_nostr_pubkey,
          callback_relays:
            Array.isArray(params.callback_relays) && params.callback_relays.length > 0
              ? asStringArray(params.callback_relays, "callback_relays")
              : asStringArray(
                  pluginConfig.defaultCallbackRelays?.length
                    ? pluginConfig.defaultCallbackRelays
                    : caps.relay_urls,
                  "callback_relays"
                ),
          items: asItems(params.items),
          settlement_preference: pickSettlementPreference(params, pluginConfig),
          buyer_reference: parseOptionalString(params.buyer_reference)
        };
        const response = await saturnSignedPost(pluginConfig, "/quote", payload);
        return textResult(response);
      }
    },
    { optional: true }
  );

  api.registerTool(
    {
      name: "saturn_create_checkout",
      description: "Create checkout intent in Saturn for a quote.",
      parameters: {
        type: "object",
        properties: {
          quote_id: { type: "string" },
          selected_rail: { type: "string", enum: ["lightning", "onchain"] },
          buyer_reference: { type: "string" },
          return_relays: { type: "array", items: { type: "string" } },
          idempotency_key: { type: "string" }
        },
        required: ["quote_id"],
        additionalProperties: false
      },
      async execute(_id, params) {
        const pluginConfig = getPluginConfig(api);
        const payload = {
          quote_id: String(params.quote_id),
          selected_rail: pickSelectedRail(params, pluginConfig),
          buyer_reference: parseOptionalString(params.buyer_reference),
          return_relays: Array.isArray(params.return_relays)
            ? asStringArray(params.return_relays, "return_relays")
            : undefined
        };

        const idempotencyKey = parseOptionalString(params.idempotency_key) || randomUUID();
        const response = await saturnSignedPost(
          pluginConfig,
          "/checkout-intent",
          payload,
          idempotencyKey
        );
        return textResult({ idempotency_key: idempotencyKey, ...response });
      }
    },
    { optional: true }
  );

  api.registerTool(
    {
      name: "saturn_confirm_payment",
      description: "Confirm payment in Saturn using a settlement proof.",
      parameters: {
        type: "object",
        properties: {
          order_id: { type: "string" },
          rail: { type: "string", enum: ["lightning", "onchain"] },
          settlement_proof: {
            type: "object",
            description: "Saturn settlement proof object (type=lightning or type=on_chain)."
          },
          idempotency_key: { type: "string" }
        },
        required: ["order_id", "rail", "settlement_proof"],
        additionalProperties: false
      },
      async execute(_id, params) {
        const pluginConfig = getPluginConfig(api);
        const payload = {
          order_id: String(params.order_id),
          rail: String(params.rail),
          settlement_proof: asObject(params.settlement_proof, "settlement_proof")
        };
        const idempotencyKey = parseOptionalString(params.idempotency_key) || randomUUID();
        const response = await saturnSignedPost(
          pluginConfig,
          "/payment/confirm",
          payload,
          idempotencyKey
        );
        return textResult({ idempotency_key: idempotencyKey, ...response });
      }
    },
    { optional: true }
  );

  api.registerTool(
    {
      name: "saturn_get_order",
      description: "Fetch Saturn order state and payment status by order id.",
      parameters: {
        type: "object",
        properties: {
          order_id: { type: "string" }
        },
        required: ["order_id"],
        additionalProperties: false
      },
      async execute(_id, params) {
        const pluginConfig = getPluginConfig(api);
        const orderId = String(params.order_id);
        const response = await saturnRequest(pluginConfig, "GET", `/order/${orderId}`);
        return textResult(response);
      }
    },
    { optional: true }
  );
  api.registerTool(
    {
      name: "saturn_fulfill_order",
      description: "Mark a paid Saturn order as fulfilled.",
      parameters: {
        type: "object",
        properties: {
          order_id: { type: "string" }
        },
        required: ["order_id"],
        additionalProperties: false
      },
      async execute(_id, params) {
        const pluginConfig = getPluginConfig(api);
        const orderId = String(params.order_id);
        const payload = { order_id: orderId };
        const response = await saturnSignedPost(
          pluginConfig,
          `/order/${orderId}/fulfill`,
          payload
        );
        return textResult(response);
      }
    },
    { optional: true }
  );
}
