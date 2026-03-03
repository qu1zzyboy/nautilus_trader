# Pull Request

**NautilusTrader prioritizes correctness and reliability, please follow existing patterns for validation and testing.**

- [ ] I have reviewed the `CONTRIBUTING.md` and followed the established practices

## Summary

This PR implements the **short-term solution** suggested in [issue #3664](https://github.com/nautechsystems/nautilus_trader/issues/3664) (Binance algo order update):

- **Algo order cleanup:** Binance sends `ORDER_TRADE_UPDATE` (fill) before `ALGO_ORDER_UPDATE` Finished. Previously, fully filled orders were removed from `active_orders` in `handle_trade_fill`, so algo orders were cleaned up too early and could be removed twice (once on fill, once on Finished). Algo orders (those in `algo_client_order_ids`) are no longer removed from `active_orders` in `handle_trade_fill`; cleanup is done only in `handle_algo_update` when the algo lifecycle ends (Finished / Canceled / Expired / Rejected).
- **Empty string parsing:** When Binance sends an empty string for the venue/order id in algo payloads, the adapter no longer panics on `VenueOrderId::new("")`. Empty values are handled by using the algo_id as a placeholder (or skipping as appropriate) so parsing and lookups remain correct.

## Related Issues/PRs

- Related to [#3664](https://github.com/nautechsystems/nautilus_trader/issues/3664) — [Binance] Algo order update

## Type of change

- [x] Bug fix (non-breaking)
- [ ] New feature (non-breaking)
- [ ] Improvement (non-breaking)
- [ ] Breaking change (impacts existing behavior)
- [ ] Documentation update
- [ ] Maintenance / chore

## Breaking change details (if applicable)

N/A

## Documentation

- [ ] Documentation changes follow the style guide (`docs/developer_guide/docs.md`)

## Release notes

- [ ] I added a concise entry to `RELEASES.md` that follows the existing conventions (when applicable)

## Testing

- [x] Affected code paths are already covered by the test suite
- [ ] I added/updated tests to cover new or changed logic

Manual: Binance Futures algo orders (e.g. STOP_MARKET / StopLimit) were tested; with fill events arriving before `ALGO_ORDER_UPDATE` Finished, active_orders no longer has duplicate removal or failed lookups. Empty venue id in payloads no longer causes a panic.
