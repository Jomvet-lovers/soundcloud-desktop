# CPU-образ воркера. Тэг: ghcr.io/<repo>/worker:cpu
#
# Запускается на любом x86_64 хосте без GPU. Все модели (Whisper, Demucs, MuQ,
# bge-m3, Qwen) крутятся на CPU — в разы медленнее, чем на GPU, но работает.
# Берут этот образ для дешёвых дополнительных consumer'ов, dev-стендов или
# хостов, где GPU физически нет.
#
# Никаких CUDA-требований к хосту нет — образ полностью самодостаточный.
FROM python:3.12-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ffmpeg libsndfile1 libchromaprint-tools \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
# torch / torchaudio / torchvision / torchcodec — одной командой с CPU-индекса
# pytorch.org, чтобы pip взял согласованные версии всех четырёх.
# torchcodec обязателен с torchaudio 2.8+: `torchaudio.load()` теперь декодит
# через TorchCodec, без него падает `TorchCodec is required for load_with_torchcodec`.
# В requirements.txt torch-семейство не указано, чтобы не было апгрейд-гонки.
RUN pip install --no-cache-dir \
        --index-url https://download.pytorch.org/whl/cpu \
        torch torchaudio torchvision torchcodec \
 && pip install --no-cache-dir -r requirements.txt

COPY src ./src

ENV HF_HOME=/models
ENV PYTHONUNBUFFERED=1
VOLUME ["/models"]

CMD ["python", "-m", "src.main"]
