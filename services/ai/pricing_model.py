#!/usr/bin/env python3
"""Sentinel pricing simulator.

Models per-user AI cost (COGS) and gross margin for two delivery modes:

  • MANAGED   — we run the AI sidecar on RunPod and pay for inference.
  • SELF-HOST — the user runs the sidecar (`task ai`) on their own machine or
                cloud (AI_SERVICE_URL → localhost / their box). Our AI COGS ≈ $0,
                so we can charge less.

Edit the ASSUMPTIONS block and run:  python3 services/ai/pricing_model.py
All inputs are rough — replace GPU_RATE with your endpoint's actual $/sec.
"""

# ----------------------------- ASSUMPTIONS --------------------------------
DAYS = 30

# RunPod flex (scale-to-zero) — check your endpoint for the real per-second rate.
GPU_RATE = 0.00020      # $/sec for one worker (≈ low/mid GPU)
EXEC_S = 0.5            # warm execution seconds per AI call (you measured ~0.5)
COLD_S = 10.0          # extra billed seconds when a call hits a cold start
COLD_FRAC = 0.05       # share of calls that cold-start (high when few users,
                       # → ~0 at scale with a shared warm pool)

# Non-AI platform COGS we pay regardless of mode (Anthropic chat/vision +
# ElevenLabs voice, averaged per active user per month). Tune to taste.
PLATFORM_COGS = 0.75

# Payment processing (Stripe-style).
STRIPE_PCT = 0.029
STRIPE_FLAT = 0.30

# Usage scenarios (avg AI calls/user/day) and price points to test.
CALL_SCENARIOS = [50, 200, 500, 1000]
MANAGED_PRICES = [19, 29, 39]
SELFHOST_PRICES = [7, 9, 12]

# Shared warm-pool view (the scale story): W warm workers serve N users.
POOL_USERS = 1000
POOL_WARM_WORKERS = 2
POOL_AVG_CALLS = 200
# --------------------------------------------------------------------------


def ai_cogs_flex(calls_per_day: float) -> float:
    """Per-user monthly AI cost on flex/scale-to-zero (exec + cold starts)."""
    calls = calls_per_day * DAYS
    exec_cost = calls * EXEC_S * GPU_RATE
    cold_cost = calls * COLD_FRAC * COLD_S * GPU_RATE
    return exec_cost + cold_cost


def ai_cogs_pool(calls_per_day: float, n_users: int, warm_workers: int) -> float:
    """Per-user AI cost when `warm_workers` are kept warm and shared by N users.

    No cold starts; you pay for idle warm capacity, amortized across users, plus
    the raw execution time.
    """
    warm = warm_workers * GPU_RATE * 86_400 * DAYS / n_users
    exec_cost = calls_per_day * DAYS * EXEC_S * GPU_RATE
    return warm + exec_cost


def margin(price: float, cogs: float):
    """Return (gross_profit_$, gross_margin_fraction) after payment fees."""
    net = price * (1 - STRIPE_PCT) - STRIPE_FLAT
    profit = net - cogs
    return profit, (profit / price if price else 0.0)


def hr(title=""):
    print(f"\n{'─' * 64}")
    if title:
        print(title)
        print("─" * 64)


def main():
    print("SENTINEL PRICING SIMULATOR")
    print(f"GPU ${GPU_RATE}/s · {EXEC_S}s/call · cold {COLD_S}s @ {COLD_FRAC:.0%} · "
          f"platform ${PLATFORM_COGS}/user · Stripe {STRIPE_PCT:.1%}+${STRIPE_FLAT}")

    # --- Managed: AI COGS per user by usage ---
    hr("MANAGED — per-user monthly COGS (flex / scale-to-zero)")
    print(f"{'calls/day':>10} {'AI cost':>9} {'+platform':>10} {'= COGS':>8}")
    cogs_by_calls = {}
    for c in CALL_SCENARIOS:
        ai = ai_cogs_flex(c)
        total = ai + PLATFORM_COGS
        cogs_by_calls[c] = total
        print(f"{c:>10} {ai:>8.2f}$ {PLATFORM_COGS:>9.2f}$ {total:>7.2f}$")

    # --- Managed: margin at price points ---
    hr("MANAGED — gross margin by price × usage")
    header = f"{'calls/day':>10}" + "".join(f"{'$'+str(p):>14}" for p in MANAGED_PRICES)
    print(header)
    for c in CALL_SCENARIOS:
        row = f"{c:>10}"
        for p in MANAGED_PRICES:
            profit, frac = margin(p, cogs_by_calls[c])
            row += f"  {profit:>5.1f}$ {frac:>4.0%}"
        print(row)
    print("\n(target SaaS gross margin ≈ 70–80%)")

    # --- Self-host: AI COGS ≈ 0 (user runs the sidecar) ---
    hr("SELF-HOST — user runs the AI; our AI COGS = $0")
    sh_cogs = PLATFORM_COGS  # only the non-AI platform cost remains
    print(f"per-user COGS = ${sh_cogs:.2f} (platform only)\n")
    print(f"{'price':>8} {'profit':>9} {'margin':>8}")
    for p in SELFHOST_PRICES:
        profit, frac = margin(p, sh_cogs)
        print(f"{'$'+str(p):>8} {profit:>8.2f}$ {frac:>7.0%}")

    # --- Scale story: shared warm pool ---
    hr("SCALE — shared warm pool (no per-user cold starts)")
    pool = ai_cogs_pool(POOL_AVG_CALLS, POOL_USERS, POOL_WARM_WORKERS)
    warm_per_user = POOL_WARM_WORKERS * GPU_RATE * 86_400 * DAYS / POOL_USERS
    print(f"{POOL_WARM_WORKERS} warm worker(s) shared by {POOL_USERS} users "
          f"@ {POOL_AVG_CALLS} calls/day:")
    print(f"  warm capacity  = ${warm_per_user:.2f}/user")
    print(f"  + execution    = ${pool - warm_per_user:.2f}/user")
    print(f"  = AI COGS      = ${pool:.2f}/user  (vs flex "
          f"${ai_cogs_flex(POOL_AVG_CALLS):.2f})")
    p = MANAGED_PRICES[0]
    profit, frac = margin(p, pool + PLATFORM_COGS)
    print(f"  at ${p}: margin {frac:.0%}  → cold-start cost vanishes at scale")

    hr("TAKEAWAYS")
    print("• Self-host plan can be cheap AND high-margin (≈$0 AI COGS).")
    print("• Managed margin is thin only for a lone heavy user on flex; a shared")
    print("  warm pool at scale collapses cold-start cost.")
    print("• Cap fair-use on the managed plan so one power user can't erase margin.")


if __name__ == "__main__":
    main()
