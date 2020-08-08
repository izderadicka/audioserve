# nginx

This is my current configuration:

```
   location /audioserve/ {
        proxy_set_header X-Real-IP         $remote_addr;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host  $http_host;
        proxy_set_header Host              $http_host;
        proxy_max_temp_file_size           0;
        proxy_pass                         http://127.0.0.1:3000/;
        proxy_redirect                     http:// https://;
        proxy_read_timeout 1200s;
        send_timeout 1200s;
        }

   location /audioserve/position {
      proxy_pass http://localhost:3000/position;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_read_timeout 3600s;
        }

```