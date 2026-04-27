FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ffmpeg libsndfile1 \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY src ./src

ENV HF_HOME=/models
ENV PYTHONUNBUFFERED=1
VOLUME ["/models"]

CMD ["python", "-m", "src.main"]
