# Easy Guide To Deploy Audioserve

This guide give you receipt how to deploy audioserve easily and quickly without any special IT skills - just basic command line and common Internet tools knowledge is enough. It's opinionated to **Ubuntu**, other deployments are of course possible. This guide tries to keeps it simple, with minimal dependencies and tools - all you need is just one (virtual) host with Ubuntu Linux, which have public IP address and DNS name.

You'll end up with fully working audioserve, securely accessible from Internet, serving your favorite audiobooks to you and family and friends (indeed you need to **consider authors rights before sharing**). All setup is free - no initial and recurring costs (depending on particular choices, what I'm describing here is now completely free, but depends of some current free offerings).

## Prerequisites

1. Virtual or physical machine running Linux (Ubuntu) with public IP address - it should have public IP address
2. Domain name - 2nd ot 3rd level domain where you can set to IP from previous item

## Setup

### Virtual/Physical Machine

Install machine with latest Ubuntu (20.4, 18.4) (I used current [offering from Oracle](https://www.oracle.com/cloud/free/)).  Assure you have SSH access to the machine (If you need to learn more about SSH try [this free course](https://www.udemy.com/course/ssh-basics-for-cloud-security/)).

When logged into host you need to install Docker:  either follow [official guide](https://docs.docker.com/engine/install/ubuntu/) to get latest and greatest Docker or I just installed bundled version via `sudo apt update && sudo apt install docker.io`, it was enough. You'll also need to add your user to docker group with `sudo usermod -a -G docker $(whoami)` and then restart ssh connection for change to take effect.

Now assure that host has public IP address and address has valid DNS record. (if you do not have domain you can you use free DDNS services like dynu.com or afraid.org - if need to know more about setting free DDNS domain try [this guide](https://www.howtogeek.com/66438/how-to-easily-access-your-home-network-from-anywhere-with-ddns/).

Assure host ports 80 and 433 are accessible from Internet (either cloud provider (cloud hosted) or your home router (home hosted) may need some additional settings).

### Docker containers

Now you need basically start two services:
- reverse proxy - **nginx** - to terminate TLS (secure encrypted connection - https) and protect you from various internet attacks, as nginx is much more battle proven then audioserve (and you can tighten security there with additional settings, not covered here). In our case there is also one companion service to assure nginx has appropriate certificate for TLS(https) serving. 
- **audioserve** itself, servings files from your collection directories

So create two simple scripts (you can start directly typing to shell, but having a script file is useful for future usage) - you'll need to replace there couple of parameters - marked there as `<placeholder_name>`:

run-nginx.sh:

```bash
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
    --env "DEFAULT_EMAIL=<your_email>" \
    jrcs/letsencrypt-nginx-proxy-companion
```
Above will start nginx reverse proxy, which will automatically configure itself as the frontend for other started containers (assuming they contain proper environment variables). Edit this file to add your email you want to use with [Let's Encrypt Certification Authority](https://letsencrypt.org/).

And then create another script to start audioserve:

run-audioserve.sh
```bash
docker run -d --name audioserve  \
    -v $HOME/audiobooks:/audiobooks \
    -v $HOME/.audioserve:/home/audioserve/.audioserve \
    -e AUDIOSERVE_SHARED_SECRET=<your_shared_secret> \
    -e VIRTUAL_HOST=<your_host_name> \
    -e LETSENCRYPT_HOST=<your_host_name>  \
    --restart unless-stopped \
    izderadicka/audioserve \
    --search-cache \
	--transcoding-max-parallel-processes 24 \
    /audiobooks

```

Do not forget to make these two scripts executable with `chmod a+x *.sh` command.

You will need to replace `<you_host_name>` with domain name added to DNS in previous step. Also you need to create two directories `mkdir $HOME/audiobooks` (audiobooks collection directory), which must be readable for audioserve container, and `mkdir $HOME/.audioserve`, which must be writable and readable for audioserve container. Audioserve container is running by default with user and group id 1000 (which is default regular user in many distributions, so it usually works without further considerations). If you have different uid (check by `id` command), you will need to assure that audioserve has proper access (either `chmod` on directories or run audioserve container with different uid).

## Ramp up

Now you should have running (virtual) host, with ubuntu and docker, this host should have valid DNS name (check by trying to ssh there with host name) and open ports 80 and 443. 

Run script `./run-nginx.sh` and wait until it starts fully.  Then try to load in browser `http://your.host.name` and you should get page with "503 Service Temporarily Unavailable" error as audioserve is not yet running.

Run script `./run-audioserve.sh` and wait a bit (you can check `docker logs -f nginx-letsencrypt` to see that certificate was installed) then reload browser and you should see audioserve login screen - log there with your shared secret.  

There are no audio files to listen to - so let's copy there some in next step.

## Copying audiobooks to host

In order to test (and further use) audioserve you'll need to copy some audiobooks to collections directory. As ssh connection is working you can use `scp` command - something like `scp -r ./my_new_audiobook me@remote.host:audiobooks/`. If you want GUI application [Filezilla](https://filezilla-project.org/) is nice application for copying files using SFTP protocol (usually supported in SSH daemon).
Another solution could be to synchronize local collection to remote server with `rsync` command, that supports file copy one SSH too.

After copying files assure that they are readable for user that runs audioserve (id 1000 by default) and then just navigate to new audiobooks in web client (reload Home, if you cannot see them).

## Considerations

Above steps worked for me, but your setup might be bit different.  There are couple of items to check particularly, if you run into problems:
- ports 80 and 443 are open
- Domain works with Let's Encrypt service (check their [docs](https://letsencrypt.org/docs/) or (help forum)[https://community.letsencrypt.org/])
- you have proper permission on files and directories used by audioserve

This guide describes just simple straightforward installation - check [README file](../README.md) for more details about advanced features of audioserve and other deployment option. 






