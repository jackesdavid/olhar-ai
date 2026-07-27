// Client hooks (card #146). Installs the global error reporter (window.onerror +
// unhandledrejection → control → Datadog `service:app`) and reports SvelteKit's own
// render/navigation errors through the same channel.
import type { HandleClientError } from "@sveltejs/kit";

import { installErrorReporter, reportClientError } from "$lib/errors";

installErrorReporter();

export const handleError: HandleClientError = ({ error, event }) => {
  const err = error as Error;
  reportClientError({
    kind: "sveltekit",
    message: err?.message ?? String(error),
    stack: err?.stack,
    source: event?.url?.pathname,
  });
  // Fall through to SvelteKit's default rendering of the error page.
  return { message: err?.message ?? "An unexpected error occurred." };
};
