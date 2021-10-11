docker run -d --name audioserve  \
        -v $HOME/audiobooks/test_audiobooks:/audiobooks \
        -v $HOME/.audioserve:/home/audioserve/.audioserve \
        -e AUDIOSERVE_SHARED_SECRET=VerySecretPasswordIndeed \
        -e VIRTUAL_HOST=audioserve.example.com \
        -e LETSENCRYPT_HOST=audioserve.example.com  \
        --restart unless-stopped \
        izderadicka/audioserve \
	--transcoding-max-parallel-processes 24 \
        /audiobooks
