# cachedirtree

A library to cache directory structure into memory (arena based tree structure) to enable
fast searches over directory structure.

Search for "word1 word2 word3" will find pathes that have all these words somewhere along the path.
Also can monitor directory and updated cache after something changes in directory structure.

Work in progress - use at your own risk - no documentation yet.

See examples/watch_dir for sample usage:

```shell
cargo run --release --example watch_dir -- test_data &
echo doyle modry | nc localhost 54321
mkdir -p test_data/my/fresh/new
# wait a while
echo my fresh | nc localhost 54321
echo my new | nc localhost 54321
rm -r test_data/my
# wait a while
echo my new | nc localhost 54321
kill %1
```