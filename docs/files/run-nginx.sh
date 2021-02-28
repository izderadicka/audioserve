docker run -d -p 80:80 -p 443:443 \
    --name nginx \
    -v /var/run/docker.sock:/tmp/docker.sock:ro \
    --volume /etc/nginx/certs \
    --volume /etc/nginx/vhost.d \
    --volume /usr/share/nginx/html \
    --restart  unless-stopped \
    jwilder/nginx-proxy

docker run --detach \
    --restart unless-stopped \
    --name nginx-letsencrypt \
    --volumes-from nginx \
    --volume /var/run/docker.sock:/var/run/docker.sock:ro \
    --volume /etc/acme.sh \
    --env "DEFAULT_EMAIL=someone@example.com" \
    jrcs/letsencrypt-nginx-proxy-companion
