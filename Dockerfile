FROM archlinux:latest
WORKDIR /app

RUN curl -fsSL -o app.tar.gz \
    https://github.com/Vikkernez18/Space-App-X86/releases/download/v1.0.0/Space-App-X86-Linux-x86_64.tar.gz \
    && tar -xzf app.tar.gz && rm app.tar.gz \
    && if [ ! -f ./analog_server ]; then mv */* . 2>/dev/null || true; fi \
    && chmod +x ./analog_server

EXPOSE 10000
CMD ["sh", "-c", "export BIND_ADDR=0.0.0.0:${PORT:-10000} SERVER_ADDR=0.0.0.0:${PORT:-10000}; exec ./analog_server"]
