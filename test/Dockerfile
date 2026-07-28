FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install ocserv and tools
RUN apt-get update && apt-get install -y \
    ocserv \
    openssl \
    iptables \
    && rm -rf /var/lib/apt/lists/*

# Directories
RUN mkdir -p /etc/ocserv /var/run/ocserv

# Copy entrypoint
COPY docker-entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 4443/tcp
EXPOSE 4443/udp

ENTRYPOINT ["/entrypoint.sh"]
