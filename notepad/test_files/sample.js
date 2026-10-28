#!/usr/bin/env node
// Sample JavaScript file for syntax highlighting and first-line detection

/* Multi-line comment block
   testing JavaScript context transitions */
export async function fetchMetrics(endpoint, retries = 3) {
    const headers = { "Content-Type": "application/json" };
    for (let attempt = 1; attempt <= retries; attempt++) {
        try {
            const payload = JSON.stringify({ attempt, active: true });
            console.log(`Requesting ${endpoint} with ${payload}`);
            return { ok: true, status: 200, count: 42 };
        } catch (err) {
            if (attempt === retries) {
                throw err;
            }
        }
    }
    return null;
}
