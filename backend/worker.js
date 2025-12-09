// Eidetic License Verification Worker (Freemius Integration)
//
// Configuration:
// 1. `npx wrangler kv:namespace create "LICENSES"`
// 2. Add binding to wrangler.toml: [[kv_namespaces]] binding = "LICENSES" id = "..."
// 3. Set 'FREEMIUS_SECRET_KEY' in Cloudflare Worker secrets: `npx wrangler secret put FREEMIUS_SECRET_KEY`

export default {
    async fetch(request, env, ctx) {
        const url = new URL(request.url);

        // 1. Freemius Webhook Handler
        // POST /webhook
        if (url.pathname === "/webhook" && request.method === "POST") {
            try {
                // Get raw body as text for signature verification
                const rawBody = await request.text();
                const signature = request.headers.get("x-signature") || "";

                // Verify Signature if Secret Key is set
                // (In prod, ALWAYS set this. In dev, you might skip if testing with simple tools)

                if (env.FREEMIUS_SECRET_KEY) {
                    const encoder = new TextEncoder();
                    const key = await crypto.subtle.importKey(
                        "raw",
                        encoder.encode(env.FREEMIUS_SECRET_KEY),
                        { name: "HMAC", hash: "SHA-256" },
                        false,
                        ["verify"]
                    );

                    const verified = await crypto.subtle.verify(
                        "HMAC",
                        key,
                        hexStringToArrayBuffer(signature),
                        encoder.encode(rawBody)
                    );

                    if (!verified) {
                        return new Response("Invalid Signature", { status: 401 });
                    }
                }


                const payload = JSON.parse(rawBody);

                // Event: license.created (or similar, depending on Freemius exact event name for new subs)
                // We mainly want to track when a user buys so we have a record.
                // Freemius sends strict payloads. We look for 'install' or 'license' objects.

                // Example: storing based on license ID or user email
                let licenseKey = payload.license ? payload.license.secret_key : null;
                let userEmail = payload.user ? payload.user.email : null;
                let eventType = payload.event_type || payload.type || "unknown"; // Freemius sends 'type'

                if (licenseKey && userEmail) {
                    const record = {
                        email: userEmail,
                        freemius_id: payload.license ? payload.license.id : "unknown",
                        plan_id: payload.install ? payload.install.plan_id : "unknown",
                        status: eventType,
                        updated_at: Date.now()
                    };

                    // Store in KV for our records
                    await env.LICENSES.put(licenseKey, JSON.stringify(record));
                    console.log(`[Freemius] Stored record for ${userEmail} (${eventType})`);
                }

                return new Response("Webhook Received", { status: 200 });

            } catch (err) {
                return new Response(`Error processing webhook: ${err.message}`, { status: 400 });
            }
        }

        return new Response("Eidetic Backend: Webhook Endpoint Ready");
    },
};

// Helper to convert hex string to ArrayBuffer for crypto API
function hexStringToArrayBuffer(hexString) {
    const result = new Uint8Array(hexString.length / 2);
    for (let i = 0; i < hexString.length; i += 2) {
        result[i / 2] = parseInt(hexString.substring(i, i + 2), 16);
    }
    return result.buffer;
}
