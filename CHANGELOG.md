# Changelog

All notable changes to CarbonChain are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Added

**Smart Contracts**
- `credit_registry`: verifier management (`register_verifier`, `remove_verifier`, `list_verifiers`), `approve_and_mint`, `flag_credit`, `mark_retired`, `get_credit`, `list_credits_by_project` — 11 tests
- `retirement`: `retire` with cross-contract `mark_retired` call, `get_retirement`, `get_retirements_by_account` — 3 tests
- `marketplace`: `create_offer`, `cancel_offer`, `get_offer`, `get_offers_by_seller`, `offer_count` — 7 tests
- `mrv_oracle`: `initialize`, `register_oracle`, `update_mrv_data` with >20% anomaly detection, `get_latest`, `get_history` — 5 tests

**API (NestJS)**
- `VerifiersModule` — `GET /verifiers`, `GET /verifiers/:address`
- `RetirementModule` — `POST /retirement`, `GET /retirement/:id`, `GET /retirement/account/:address`
- `MarketplaceModule` — `POST /marketplace/offer`, `GET /marketplace/offer/:id`, `GET /marketplace/seller/:address`, `DELETE /marketplace/offer/:id/seller/:address`
- `CreditsModule` — `POST /credits/issue`
- SEP-10 auth (`/auth/challenge`, `/auth/token`, `/auth/me`)

**Frontend (Angular)**
- `AuthService` — full SEP-10 wallet login flow
- `ConnectWalletComponent` — Freighter connect button with install prompt
- Marketplace page (`/marketplace`) — browse and display active listings
- Retire wizard (`/retire`) — 3-step credit retirement form
- Angular dev proxy — `/api` routes to `http://localhost:3000`

### Open for contributors
- Buy / fill offer flow (contract + API + UI)
- Browse all marketplace listings (requires `list_all_offers` contract function)
- Retirement certificate viewer (`/certificates/:id`)
- Admin panel — verifier approval and credit minting UI
- JWT auth guards on state-mutating API endpoints
- `docker-compose.yml` for local PostgreSQL
- `deploy-testnet.sh` implementation
- Mobile-responsive layout
