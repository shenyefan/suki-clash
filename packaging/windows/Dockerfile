ARG RUST_VERSION=1.99.1
FROM suki-clash-build-linux:${RUST_VERSION}

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends wine wine64-tools libwine-dev cabextract \
    && rm -rf /var/lib/apt/lists/*
ENV WINEPREFIX=/opt/suki-wine \
    WINEARCH=win64 \
    WINEDEBUG=-all \
    WINEDLLOVERRIDES=d3dcompiler_47=n
COPY shader-compiler.c /opt/shader-compiler.c
COPY compile-shaders.py /opt/compile-shaders.py
RUN winegcc -m64 -O2 /opt/shader-compiler.c -o /opt/shader-compiler.exe \
    && wineboot --init \
    && wineserver --wait
RUN python3 -c "import hashlib, urllib.request; data = urllib.request.urlopen('https://download.microsoft.com/download/B/0/C/B0C80BA3-8AD6-4958-810B-6882485230B5/standalonesdk/Installers/61d57a7a82309cd161a854a6f4619e52.cab', timeout=120).read(); assert hashlib.sha256(data).hexdigest() == 'f736e161547095bb8d98c636b85fdfeb4070fefeee3c3745db3ce88f6eb1d9de', 'Microsoft compiler archive SHA256 mismatch'; open('/tmp/d3dcompiler.cab', 'wb').write(data)" \
    && cabextract -q -d /tmp -F fil3585cb2ea5db13cc0838f8d06b5c9679 /tmp/d3dcompiler.cab \
    && mv /tmp/fil3585cb2ea5db13cc0838f8d06b5c9679 /opt/suki-wine/drive_c/windows/system32/d3dcompiler_47.dll \
    && rm /tmp/d3dcompiler.cab
