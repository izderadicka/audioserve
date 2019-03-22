async-tar
=========

This crate provides a way to create tar archive in an asynchronous way.

`TarStream` is a `Stream` that provides chunks (`Vec<u8>`) of asynchronously created tar archive.
