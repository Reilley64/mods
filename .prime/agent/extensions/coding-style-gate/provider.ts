import { TypeSafeClient } from "@typesafe-ai/sdk";
import type { GateConfig } from "./config";

// Preserve the verified OpenRouter cache identity; direct-backend entries cannot match.
export function backendIdentity(): string {
	return "openrouter:https://openrouter.ai/api/v1/systemone:sdk-0.6.0:mapping-20260917-v1";
}

export function responseModelMatches(requested: string, actual: unknown): boolean {
	return actual === requested || (requested === "typesafe/jev-1.13"
		&& actual === "typesafe/jev-1.13-20260917");
}

export function createReviewClient(config: Pick<GateConfig, "model" | "timeoutMs">): TypeSafeClient {
	const apiKey = process.env.OPENROUTER_API_KEY;
	if (!apiKey) throw new Error("coding-style-gate: missing OPENROUTER_API_KEY");
	return new TypeSafeClient({
		apiKey,
		baseURL: "https://openrouter.ai/api",
		defaultModel: config.model,
		logLevel: "warn",
		retry: { maxRetries: 1 },
		timeout: config.timeoutMs,
	});
}
