use std::io;

#[cfg(not(feature = "asynch"))]
quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Io(err: io::Error) {
            from()
            cause(err)
            display("io error: {}", err)
        }

        
        KeyAlreadyExists(key: String) {
            display("key {} exists", key)
        }

        InvalidKey {
            display("key is invalid - too big")
        }

        InvalidIndex {
            display("index file is invalid")
        }

        FileTooBig {
            display("file bigger then max cache size")
        }

        KeyOpened(key: String) {
            display("key {} is being added", key)
        }

        InvalidCacheState(reason: String) {
            display("invalid cache state: {}", reason)
        }
    }

    
}


#[cfg(feature = "asynch")]
quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Io(err: io::Error) {
            from()
            cause(err)
            display("io error: {}", err)
        }

        
        Pool(err: tokio_threadpool::BlockingError) {
            from()
            cause(err)
            display("pool error: {}", err)
        }

        KeyAlreadyExists(key: String) {
            display("key {} exists", key)
        }

        InvalidKey {
            display("key is invalid - too big")
        }

        InvalidIndex {
            display("index file is invalid")
        }

        FileTooBig {
            display("file bigger then max cache size")
        }

        KeyOpened(key: String) {
            display("key {} is being added", key)
        }

        InvalidCacheState(reason: String) {
            display("invalid cache state: {}", reason)
        }
    }
}