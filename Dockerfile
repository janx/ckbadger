FROM ubuntu:24.04

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates libssl3t64 libstdc++6 zlib1g \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 1000 -g 1000 /work

COPY --chmod=0755 ckbadger /usr/local/bin/ckbadger

USER 1000:1000
WORKDIR /work
# The supervisor handles SIGINT for cooperative shutdown.
STOPSIGNAL SIGINT
ENTRYPOINT ["ckbadger"]
CMD ["run"]
