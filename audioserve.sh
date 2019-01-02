#!/usr/bin/env bash
ARGS="${DIRS} --shared-secret ${SECRET}"
if [[ ! -z "${SSLKEY}" ]]; then
	ARGS="${ARGS} --ssl-key ${SSLKEY}"
fi

if [[ ! -z "${SSLPASS}" ]]; then
	ARGS="${ARGS} --ssl-key-password ${SSLPASS}"
fi

if [[ ! -z "${PORT}" ]]; then
	ARGS="${ARGS} --listen 0.0.0.0:${PORT}"
fi

if [[ ! -z "${OTHER_ARGS}" ]]; then
	ARGS="${ARGS} ${OTHER_ARGS}"
fi

./audioserve ${ARGS} 
