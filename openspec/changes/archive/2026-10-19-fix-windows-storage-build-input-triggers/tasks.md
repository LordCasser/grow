## 1. Specification-first change record

- [x] 1.1 Read the development workflow contract, current changes, and archived F4 audit evidence.
- [x] 1.2 Record the non-contract change and why no delta spec is appropriate.

## 2. Filter and regression coverage

- [x] 2.1 Add `.cargo/**` and `Cargo.toml` to both pull request and `main` push path filters.
- [x] 2.2 Add Python checks that fail if either trigger omits a direct build input, trigger filters diverge, or release no longer calls and requires the reusable Windows gate.
- [x] 2.3 Run the relevant Python tests and strict OpenSpec validation; record adversarial review and results.
