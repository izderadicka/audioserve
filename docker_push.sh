#!/bin/bash
set -e
echo "$DOCKER_HUB_PASSWORD" | docker login -u "$DOCKER_HUB_USER" --password-stdin
docker build --tag izderadicka/audioserve .
docker push izderadicka/audioserve