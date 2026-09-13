Merging this releases @teasel/parser@0.0.8. Changesets that land on main meanwhile are added to it.

# Releases

## @teasel/parser@0.0.8

### Patch Changes

- [#94](https://github.com/Nic-Polumeyv/teasel/pull/94) [`e6c072e`](https://github.com/Nic-Polumeyv/teasel/commit/e6c072e9bd2e98730dfb50669b4e5a66c400352e) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - non-ASCII identifier characters are looked up in a three-level bitmap trie instead of a binary search over ranges: 2.4x faster in the lexer, 1.7x in the package's own identifier check, and the tables are smaller

- [#89](https://github.com/Nic-Polumeyv/teasel/pull/89) [`4ff3ce6`](https://github.com/Nic-Polumeyv/teasel/commit/4ff3ce60a249a3fe867d32a2a9f4837e2299c576) Thanks [@Nic-Polumeyv](https://github.com/Nic-Polumeyv)! - a node's comments and its root are found through a bit per node, so the nodes without cost no hash
