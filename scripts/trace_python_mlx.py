#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import tempfile
from pathlib import Path
from typing import Any

import numpy as np
import mlx.nn as nn
from mlx_audio.codec.models.mimi.mimi import _reset_kv_cache


def normalize_language(value: str) -> str:
    normalized = value.strip().lower().replace("_", "-")
    aliases = {
        "": "auto",
        "auto": "auto",
        "de": "german",
        "de-de": "german",
        "en": "english",
        "en-us": "english",
        "en-gb": "english",
        "fr": "french",
        "fr-fr": "french",
        "es": "spanish",
        "es-es": "spanish",
        "it": "italian",
        "it-it": "italian",
        "pt": "portuguese",
        "pt-br": "portuguese",
        "pt-pt": "portuguese",
        "ja": "japanese",
        "ja-jp": "japanese",
        "ko": "korean",
        "ko-kr": "korean",
        "zh": "chinese",
        "zh-cn": "chinese",
        "zh-tw": "chinese",
        "ru": "russian",
        "ru-ru": "russian",
    }
    return aliases.get(normalized, normalized)


def normalize_ref_audio(ref_audio: str, sample_rate: int) -> str:
    import soundfile as sf
    from scipy.signal import resample_poly

    source = Path(ref_audio).expanduser().resolve()
    waveform, source_sr = sf.read(str(source), always_2d=False, dtype="float32")
    waveform = np.asarray(waveform, dtype=np.float32)
    if waveform.ndim > 1:
        waveform = waveform.mean(axis=1)
    if int(source_sr) != sample_rate:
        gcd = np.gcd(int(source_sr), sample_rate)
        waveform = resample_poly(waveform, up=sample_rate // gcd, down=int(source_sr) // gcd).astype(np.float32)
        source_sr = sample_rate
    temp = tempfile.NamedTemporaryFile(prefix="qwen3_trace_ref_", suffix=".wav", delete=False)
    temp.close()
    sf.write(temp.name, waveform, int(source_sr), format="WAV", subtype="PCM_16")
    return temp.name


class TraceWriter:
    def __init__(self, trace_dir: Path, save_tensors: bool, sample_count: int) -> None:
        self.trace_dir = trace_dir
        self.save_tensors = save_tensors
        self.sample_count = sample_count
        self.trace_dir.mkdir(parents=True, exist_ok=True)
        self.events = (self.trace_dir / "trace.jsonl").open("w", encoding="utf8")
        self.tensor_index = 0

    def close(self) -> None:
        self.events.close()

    def event(self, name: str, **fields: Any) -> None:
        self.events.write(json.dumps({"name": name, **fields}, ensure_ascii=False, sort_keys=True) + "\n")
        self.events.flush()

    def tensor(self, name: str, value: Any, *, extra: dict[str, Any] | None = None) -> None:
        import mlx.core as mx

        mx.eval(value)
        original_dtype = str(value.dtype) if hasattr(value, "dtype") else "unknown"
        try:
            array = np.array(value)
        except RuntimeError:
            array = np.array(value.astype(mx.float32))
        flat = array.reshape(-1)
        entry: dict[str, Any] = {
            "shape": list(array.shape),
            "dtype": str(array.dtype),
            "original_dtype": original_dtype,
            "size": int(array.size),
            "sha256": hashlib.sha256(np.ascontiguousarray(array).view(np.uint8)).hexdigest(),
        }
        if array.size:
            if np.issubdtype(array.dtype, np.number) or np.issubdtype(array.dtype, np.bool_):
                numeric = array.astype(np.float64, copy=False)
                entry.update(
                    {
                        "min": float(np.min(numeric)),
                        "max": float(np.max(numeric)),
                        "mean": float(np.mean(numeric)),
                    }
                )
            sample = min(self.sample_count, array.size)
            entry["first"] = flat[:sample].tolist()
            entry["last"] = flat[-sample:].tolist()
            probe_indices = sorted({0, array.size // 7, array.size // 5, array.size // 3, array.size // 2, (array.size * 2) // 3, (array.size * 4) // 5, (array.size * 6) // 7, array.size - 1})
            entry["probes"] = [[int(index), flat[index].item() if hasattr(flat[index], "item") else flat[index]] for index in probe_indices]
        if extra:
            entry.update(extra)
        if self.save_tensors:
            filename = f"{self.tensor_index:05d}_{name.replace('/', '_')}.npy"
            np.save(self.trace_dir / filename, array)
            entry["file"] = filename
            self.tensor_index += 1
        self.event(name, kind="tensor", **entry)

    def ids(self, name: str, values: list[int]) -> None:
        self.event(
            name,
            kind="ids",
            len=len(values),
            sha256=hashlib.sha256(np.asarray(values, dtype=np.int64).tobytes()).hexdigest(),
            values=values,
        )


def trace_prepare_icl_generation_inputs(trace: TraceWriter, model: Any, text: str, ref_audio: Any, ref_text: str, language: str):
    import mlx.core as mx

    config = model.config.talker_config
    audio_for_spk = ref_audio
    if ref_audio.ndim == 1:
        ref_audio = ref_audio[None, None, :]
    elif ref_audio.ndim == 2:
        ref_audio = ref_audio[None, :]
    trace.tensor("prepare/ref_audio_for_tokenizer", ref_audio)

    encoder_model = model.speech_tokenizer.encoder_model
    encoder_model.encoder.reset_state()
    for cache in encoder_model.encoder_cache:
        _reset_kv_cache(cache)
    encoder = encoder_model.encoder
    xs = encoder.init_conv1d(ref_audio)
    trace.tensor("prepare/encoder_conv_layer_00", xs)
    trace.tensor("prepare/encoder_conv_init", xs)
    for layer_index, layer in enumerate(encoder.layers):
        for residual_index, residual in enumerate(layer.residuals):
            xs = residual(xs)
            trace.tensor(f"prepare/encoder_conv_layer_{layer_index:02d}_residual_{residual_index:02d}", xs)
        xs = layer.downsample(nn.elu(xs, alpha=1.0))
        trace.tensor(f"prepare/encoder_conv_layer_{layer_index:02d}_downsample", xs)
    xs = encoder.final_conv1d(nn.elu(xs, alpha=1.0))
    trace.tensor("prepare/encoder_conv_final", xs)
    trace.tensor("prepare/encoder_after_conv", xs)
    seq_len = xs.shape[-1]
    mask = mx.full((seq_len, seq_len), -mx.inf, dtype=xs.dtype)
    mask = mx.triu(mask, k=1)[None, None, :, :]
    xs = xs.swapaxes(1, 2)
    trace.tensor("prepare/encoder_before_transformer", xs)
    transformer = encoder_model.encoder_transformer.transformer
    for layer_index, (layer, cache) in enumerate(zip(transformer.layers, encoder_model.encoder_cache)):
        if layer_index == 0:
            n1 = layer.norm1(xs)
            trace.tensor("prepare/encoder_layer_00_norm1", n1.swapaxes(1, 2))
            attn = layer.self_attn(n1, cache=cache, mask=mask)
            trace.tensor("prepare/encoder_layer_00_attn", attn.swapaxes(1, 2))
            xs = xs + layer.layer_scale_1(attn)
            trace.tensor("prepare/encoder_layer_00_after_attn", xs.swapaxes(1, 2))
            n2 = layer.norm2(xs)
            trace.tensor("prepare/encoder_layer_00_norm2", n2.swapaxes(1, 2))
            mlp_fc1 = layer.gating.linear1(n2)
            trace.tensor("prepare/encoder_layer_00_mlp_fc1", mlp_fc1.swapaxes(1, 2))
            mlp_act = nn.gelu_approx(mlp_fc1)
            trace.tensor("prepare/encoder_layer_00_mlp_act", mlp_act.swapaxes(1, 2))
            mlp = layer.gating.linear2(mlp_act)
            trace.tensor("prepare/encoder_layer_00_mlp", mlp.swapaxes(1, 2))
            xs = xs + layer.layer_scale_2(mlp)
        else:
            xs = layer(xs, cache=cache, mask=mask)
        trace.tensor(f"prepare/encoder_transformer_layer_{layer_index:02d}", xs.swapaxes(1, 2))
    xs = xs.swapaxes(1, 2)
    trace.tensor("prepare/encoder_after_transformer", xs)
    xs = encoder_model.downsample(xs)
    trace.tensor("prepare/encoder_after_downsample", xs)
    ref_codes = encoder_model.quantizer.encode(xs)[:, : encoder_model.valid_num_quantizers, :]
    mx.eval(ref_codes)
    trace.tensor("prepare/ref_codes", ref_codes)
    ref_codes_array = np.array(ref_codes).astype(int)
    (trace.trace_dir / "ref_codes.json").write_text(json.dumps(ref_codes_array.tolist()), encoding="utf8")
    trace.ids("prepare/ref_codes_flat_frame_major", ref_codes_array.transpose(0, 2, 1).reshape(-1).tolist())

    ref_chat = f"<|im_start|>assistant\n{ref_text}<|im_end|>\n"
    ref_ids_list = model.tokenizer.encode(ref_chat)
    trace.ids("prepare/ref_chat_ids", ref_ids_list)
    ref_ids = mx.array(ref_ids_list)[None, :]
    ref_text_ids = ref_ids[:, 3:-2]
    trace.tensor("prepare/ref_text_ids", ref_text_ids)

    target_chat = f"<|im_start|>assistant\n{text}<|im_end|>\n<|im_start|>assistant\n"
    target_ids_list = model.tokenizer.encode(target_chat)
    trace.ids("prepare/target_chat_ids", target_ids_list)
    target_ids = mx.array(target_ids_list)[None, :]
    text_ids = target_ids[:, 3:-5]
    trace.tensor("prepare/target_text_ids", text_ids)

    tts_tokens = mx.array([[model.config.tts_bos_token_id, model.config.tts_eos_token_id, model.config.tts_pad_token_id]])
    trace.tensor("prepare/tts_token_ids", tts_tokens)
    tts_raw_embed = model.talker.get_text_embeddings()(tts_tokens)
    trace.tensor("prepare/tts_text_projection/raw", tts_raw_embed)
    tts_fc1 = model.talker.text_projection.linear_fc1(tts_raw_embed)
    trace.tensor("prepare/tts_text_projection/fc1", tts_fc1)
    tts_act = nn.silu(tts_fc1)
    trace.tensor("prepare/tts_text_projection/act", tts_act)
    tts_embeds = model.talker.text_projection.linear_fc2(tts_act)
    trace.tensor("prepare/tts_text_projection/fc2", tts_embeds)
    trace.tensor("prepare/tts_embeds", tts_embeds)
    tts_bos_embed = tts_embeds[:, 0:1, :]
    tts_eos_embed = tts_embeds[:, 1:2, :]
    tts_pad_embed = tts_embeds[:, 2:3, :]

    combined_text_ids = mx.concatenate([ref_text_ids, text_ids], axis=1)
    trace.tensor("prepare/combined_text_ids", combined_text_ids)
    text_raw_embed = model.talker.get_text_embeddings()(combined_text_ids)
    trace.tensor("prepare/text_projection/raw", text_raw_embed)
    text_fc1 = model.talker.text_projection.linear_fc1(text_raw_embed)
    trace.tensor("prepare/text_projection/fc1", text_fc1)
    text_act = nn.silu(text_fc1)
    trace.tensor("prepare/text_projection/act", text_act)
    text_embed = model.talker.text_projection.linear_fc2(text_act)
    trace.tensor("prepare/text_projection/fc2", text_embed)
    text_embed = mx.concatenate([text_embed, tts_eos_embed], axis=1)
    trace.tensor("prepare/text_embed_with_eos", text_embed)
    text_lens = text_embed.shape[1]

    first_cb_codes = ref_codes[:, 0, :]
    trace.tensor("prepare/ref_codebook0", first_cb_codes)
    ref_codec_embed = model.talker.get_input_embeddings()(first_cb_codes)
    for i in range(config.num_code_groups - 1):
        cb_codes = ref_codes[:, i + 1, :]
        ref_codec_embed = ref_codec_embed + model.talker.code_predictor.codec_embedding[i](cb_codes)
    trace.tensor("prepare/ref_codec_embed_sum", ref_codec_embed)

    codec_bos_embed = model.talker.get_input_embeddings()(mx.array([[config.codec_bos_id]]))
    codec_embed_icl = mx.concatenate([codec_bos_embed, ref_codec_embed], axis=1)
    trace.tensor("prepare/codec_embed_icl", codec_embed_icl)
    codec_lens = codec_embed_icl.shape[1]

    codec_pad_embed = model.talker.get_input_embeddings()(mx.array([[config.codec_pad_id]]))
    text_with_codec_pad = text_embed + mx.broadcast_to(codec_pad_embed, (1, text_lens, codec_pad_embed.shape[-1]))
    codec_with_text_pad = codec_embed_icl + mx.broadcast_to(tts_pad_embed, (1, codec_lens, tts_pad_embed.shape[-1]))
    icl_input_embed = mx.concatenate([text_with_codec_pad, codec_with_text_pad], axis=1)
    trace.tensor("prepare/icl_input_embed", icl_input_embed)
    trailing_text_hidden = tts_pad_embed

    language_id = None
    if language.lower() != "auto" and config.codec_language_id and language.lower() in config.codec_language_id:
        language_id = config.codec_language_id[language.lower()]
    trace.event("prepare/language", kind="metadata", language=language, language_id=language_id)

    speaker_embed = None
    if model.speaker_encoder is not None:
        from mlx_audio.tts.models.qwen3_tts.qwen3_tts import mel_spectrogram

        speaker_mel = mel_spectrogram(
            audio_for_spk,
            n_fft=1024,
            num_mels=128,
            sample_rate=24000,
            hop_size=256,
            win_size=1024,
            fmin=0,
            fmax=12000,
        )
        trace.tensor("prepare/speaker_mel", speaker_mel)
        speaker_embed = model.speaker_encoder(speaker_mel)
        trace.tensor("prepare/speaker_embed", speaker_embed)

    if language_id is None:
        codec_prefill = [config.codec_nothink_id, config.codec_think_bos_id, config.codec_think_eos_id]
    else:
        codec_prefill = [config.codec_think_id, config.codec_think_bos_id, language_id, config.codec_think_eos_id]
    trace.ids("prepare/codec_prefill_ids", [int(x) for x in codec_prefill])

    codec_prefix_embed = model.talker.get_input_embeddings()(mx.array([codec_prefill]))
    codec_prefix_suffix = model.talker.get_input_embeddings()(mx.array([[config.codec_pad_id, config.codec_bos_id]]))
    if speaker_embed is not None:
        codec_prefix_embed = mx.concatenate([codec_prefix_embed, speaker_embed.reshape(1, 1, -1), codec_prefix_suffix], axis=1)
    else:
        codec_prefix_embed = mx.concatenate([codec_prefix_embed, codec_prefix_suffix], axis=1)
    trace.tensor("prepare/codec_prefix_embed", codec_prefix_embed)

    role_embed = model.talker.text_projection(model.talker.get_text_embeddings()(target_ids[:, :3]))
    trace.tensor("prepare/role_embed", role_embed)

    pad_count = codec_prefix_embed.shape[1] - 2
    pad_embeds = mx.broadcast_to(tts_pad_embed, (1, pad_count, tts_pad_embed.shape[-1]))
    combined_prefix = mx.concatenate([pad_embeds, tts_bos_embed], axis=1)
    combined_prefix = combined_prefix + codec_prefix_embed[:, :-1, :]
    trace.tensor("prepare/combined_prefix", combined_prefix)

    input_embeds = mx.concatenate([role_embed, combined_prefix, icl_input_embed], axis=1)
    trace.tensor("prepare/input_embeds", input_embeds)
    trace.tensor("prepare/trailing_text_hidden", trailing_text_hidden)
    trace.tensor("prepare/tts_pad_embed", tts_pad_embed)
    return input_embeds, trailing_text_hidden, tts_pad_embed, ref_codes


def trace_topk(trace: TraceWriter, name: str, logits: Any, k: int) -> None:
    import mlx.core as mx

    mx.eval(logits)
    arr = np.array(logits).reshape(-1).astype(np.float64)
    k = min(k, arr.size)
    idx = np.argpartition(-arr, k - 1)[:k]
    idx = idx[np.argsort(-arr[idx])]
    trace.event(name, kind="topk", indices=idx.astype(int).tolist(), values=arr[idx].tolist())


def run(args: argparse.Namespace) -> None:
    import mlx.core as mx
    from mlx_audio.tts.utils import load_model
    from mlx_audio.utils import load_audio

    trace = TraceWriter(Path(args.trace_dir), args.save_tensors, args.sample_count)
    try:
        mx.random.seed(args.seed)
        np.random.seed(args.seed)
        trace.event(
            "run/config",
            kind="metadata",
            model=args.model_name,
            text=args.text,
            ref_audio=args.ref_audio,
            ref_text=args.ref_text,
            language=args.language,
            max_steps=args.max_steps,
            temperature=args.temperature,
            top_k=args.top_k,
            top_p=args.top_p,
            repetition_penalty=args.repetition_penalty,
            seed=args.seed,
        )

        model = load_model(args.model_name)
        ref_audio_path = normalize_ref_audio(args.ref_audio, int(model.sample_rate))
        ref_audio = load_audio(ref_audio_path, sample_rate=model.sample_rate)
        trace.tensor("input/ref_audio_loaded", ref_audio)

        input_embeds, trailing_text_hidden, tts_pad_embed, ref_codes = trace_prepare_icl_generation_inputs(
            trace, model, args.text, ref_audio, args.ref_text, args.language
        )

        target_token_count = len(model.tokenizer.encode(args.text))
        effective_max_steps = min(args.max_steps, max(75, target_token_count * 6))
        trace.event("generation/effective_max_steps", kind="metadata", target_token_count=target_token_count, effective_max_steps=effective_max_steps)

        cache = model.talker.make_cache()
        code_cache = model.talker.code_predictor.make_cache()
        generated_codes = []
        generated_code_frames: list[list[int]] = []
        generated_token_ids: list[int] = []
        config = model.config.talker_config
        eos_token_id = config.codec_eos_token_id
        suppress_tokens = [i for i in range(config.vocab_size - 1024, config.vocab_size) if i != eos_token_id]
        trailing_idx = 0

        for step in range(effective_max_steps):
            if True:
                talker_model = model.talker.model
                batch, seq_len, _ = input_embeds.shape
                offset = cache[0].offset if cache and cache[0] is not None else 0
                pos = mx.arange(offset, offset + seq_len)[None, :].astype(mx.int32)
                pos = mx.broadcast_to(pos, (batch, seq_len))
                position_ids = mx.stack([pos, pos, pos], axis=0)
                position_embeddings = talker_model.rotary_emb(input_embeds, position_ids)
                mask = None
                if seq_len > 1:
                    mask = nn.MultiHeadAttention.create_additive_causal_mask(seq_len).astype(input_embeds.dtype)
                hidden = input_embeds
                for layer_index, layer in enumerate(talker_model.layers):
                    if layer_index <= 4:
                        detail_prefix = f"generation/step_{step:04d}/talker_layer_{layer_index:02d}_detail"
                        residual = hidden
                        norm1 = layer.input_layernorm(hidden)
                        trace.tensor(f"{detail_prefix}/norm1", norm1[:, -1:, :])
                        from mlx_audio.tts.models.qwen3_tts.talker import apply_multimodal_rotary_pos_emb
                        bsz, layer_seq_len, _ = norm1.shape
                        q = layer.self_attn.q_proj(norm1).reshape(bsz, layer_seq_len, layer.self_attn.num_heads, layer.self_attn.head_dim)
                        k = layer.self_attn.k_proj(norm1).reshape(bsz, layer_seq_len, layer.self_attn.num_kv_heads, layer.self_attn.head_dim)
                        v = layer.self_attn.v_proj(norm1).reshape(bsz, layer_seq_len, layer.self_attn.num_kv_heads, layer.self_attn.head_dim)
                        q = layer.self_attn.q_norm(q)
                        k = layer.self_attn.k_norm(k)
                        q = mx.transpose(q, (0, 2, 1, 3))
                        k = mx.transpose(k, (0, 2, 1, 3))
                        v = mx.transpose(v, (0, 2, 1, 3))
                        trace.tensor(f"{detail_prefix}/attn_detail/q_normed", q)
                        trace.tensor(f"{detail_prefix}/attn_detail/k_normed", k)
                        q_rope, k_rope = apply_multimodal_rotary_pos_emb(q, k, position_embeddings[0], position_embeddings[1])
                        trace.tensor(f"{detail_prefix}/attn_detail/q_rope", q_rope)
                        trace.tensor(f"{detail_prefix}/attn_detail/k_rope", k_rope)
                        cache_key, cache_value = cache[layer_index].update_and_fetch(k_rope, v)
                        trace.tensor(f"{detail_prefix}/attn_detail/cache_key", cache_key)
                        trace.tensor(f"{detail_prefix}/attn_detail/cache_value", cache_value)
                        sdpa = mx.fast.scaled_dot_product_attention(q_rope, cache_key, cache_value, scale=layer.self_attn.scale, mask=mask)
                        trace.tensor(f"{detail_prefix}/attn_detail/sdpa", sdpa)
                        attn = mx.transpose(sdpa, (0, 2, 1, 3)).reshape(bsz, layer_seq_len, -1)
                        attn = layer.self_attn.o_proj(attn)
                        trace.tensor(f"{detail_prefix}/attn", attn[:, -1:, :])
                        hidden = residual + attn
                        trace.tensor(f"{detail_prefix}/after_attn", hidden[:, -1:, :])
                        residual = hidden
                        norm2 = layer.post_attention_layernorm(hidden)
                        trace.tensor(f"{detail_prefix}/norm2", norm2[:, -1:, :])
                        mlp = layer.mlp(norm2)
                        trace.tensor(f"{detail_prefix}/mlp", mlp[:, -1:, :])
                        hidden = residual + mlp
                    else:
                        hidden = layer(hidden, position_embeddings, mask, cache[layer_index])
                    trace.tensor(f"generation/step_{step:04d}/talker_layer_{layer_index:02d}", hidden[:, -1:, :])
                hidden = talker_model.norm(hidden)
                trace.tensor(f"generation/step_{step:04d}/talker_norm", hidden[:, -1:, :])
                logits = model.talker.codec_head(hidden)
            trace.tensor(f"generation/step_{step:04d}/logits", logits[:, -1, :])
            trace_topk(trace, f"generation/step_{step:04d}/logits_topk", logits[:, -1, :], args.trace_topk)
            trace.tensor(f"generation/step_{step:04d}/hidden", hidden[:, -1:, :])

            next_token = model._sample_token(
                logits,
                temperature=args.temperature,
                top_k=args.top_k,
                top_p=args.top_p,
                repetition_penalty=args.repetition_penalty,
                generated_tokens=(generated_token_ids if generated_token_ids else None),
                suppress_tokens=suppress_tokens,
                eos_token_id=eos_token_id,
            )
            mx.eval(next_token)
            trace.tensor(f"generation/step_{step:04d}/code_0", next_token)
            trace.ids(f"generation/step_{step:04d}/code_0_ids", np.array(next_token).reshape(-1).astype(int).tolist())
            is_eos = bool(np.array(next_token)[0, 0] == eos_token_id)
            if is_eos:
                trace.event(f"generation/step_{step:04d}/eos", kind="metadata", eos_token_id=int(eos_token_id))
                break

            code_tokens = [next_token]
            code_hidden = hidden[:, -1:, :]
            trace.tensor(f"generation/step_{step:04d}/code_hidden", code_hidden)
            for c in code_cache:
                c.keys = None
                c.values = None
                c.offset = 0

            for code_idx in range(config.num_code_groups - 1):
                if code_idx == 0:
                    code_0_embed = model.talker.get_input_embeddings()(next_token)
                    code_input = mx.concatenate([code_hidden, code_0_embed], axis=1)
                else:
                    code_embed = model.talker.code_predictor.codec_embedding[code_idx - 1](code_tokens[-1])
                    code_input = code_embed
                trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_input", code_input)
                predictor = model.talker.code_predictor
                predictor_input = code_input
                if predictor.small_to_mtp_projection is not None:
                    predictor_input = predictor.small_to_mtp_projection(predictor_input)
                predictor_model = predictor.model
                pred_batch, pred_seq_len, _ = predictor_input.shape
                pred_offset = code_cache[0].offset if code_cache and code_cache[0] is not None else 0
                pred_position_ids = mx.arange(pred_offset, pred_offset + pred_seq_len)[None, :]
                pred_position_ids = mx.broadcast_to(pred_position_ids, (pred_batch, pred_seq_len))
                pred_position_embeddings = predictor_model.rotary_emb(predictor_input, pred_position_ids)
                pred_mask = None
                if pred_seq_len > 1:
                    pred_mask = nn.MultiHeadAttention.create_additive_causal_mask(pred_seq_len).astype(predictor_input.dtype)
                pred_hidden = predictor_input
                for pred_layer_index, pred_layer in enumerate(predictor_model.layers):
                    if pred_layer_index == 0:
                        from mlx_audio.tts.models.qwen3_tts.talker import apply_rotary_pos_emb
                        pred_residual = pred_hidden
                        pred_norm1 = pred_layer.input_layernorm(pred_hidden)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/norm1", pred_norm1)
                        pred_bsz, pred_layer_seq_len, _ = pred_norm1.shape
                        pred_q = pred_layer.self_attn.q_proj(pred_norm1).reshape(pred_bsz, pred_layer_seq_len, pred_layer.self_attn.num_heads, pred_layer.self_attn.head_dim)
                        pred_k = pred_layer.self_attn.k_proj(pred_norm1).reshape(pred_bsz, pred_layer_seq_len, pred_layer.self_attn.num_kv_heads, pred_layer.self_attn.head_dim)
                        pred_v = pred_layer.self_attn.v_proj(pred_norm1).reshape(pred_bsz, pred_layer_seq_len, pred_layer.self_attn.num_kv_heads, pred_layer.self_attn.head_dim)
                        pred_q = pred_layer.self_attn.q_norm(pred_q)
                        pred_k = pred_layer.self_attn.k_norm(pred_k)
                        pred_q = mx.transpose(pred_q, (0, 2, 1, 3))
                        pred_k = mx.transpose(pred_k, (0, 2, 1, 3))
                        pred_v = mx.transpose(pred_v, (0, 2, 1, 3))
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/q_normed", pred_q)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/k_normed", pred_k)
                        pred_q_rope, pred_k_rope = apply_rotary_pos_emb(pred_q, pred_k, pred_position_embeddings[0], pred_position_embeddings[1])
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/q_rope", pred_q_rope)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/k_rope", pred_k_rope)
                        pred_cache_key, pred_cache_value = code_cache[pred_layer_index].update_and_fetch(pred_k_rope, pred_v)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/cache_key", pred_cache_key)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/cache_value", pred_cache_value)
                        pred_sdpa = mx.fast.scaled_dot_product_attention(pred_q_rope, pred_cache_key, pred_cache_value, scale=pred_layer.self_attn.scale, mask=pred_mask)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn_detail/sdpa", pred_sdpa)
                        pred_attn = mx.transpose(pred_sdpa, (0, 2, 1, 3)).reshape(pred_bsz, pred_layer_seq_len, -1)
                        pred_attn = pred_layer.self_attn.o_proj(pred_attn)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/attn", pred_attn)
                        pred_hidden = pred_residual + pred_attn
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/after_attn", pred_hidden)
                        pred_residual = pred_hidden
                        pred_norm2 = pred_layer.post_attention_layernorm(pred_hidden)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/norm2", pred_norm2)
                        pred_mlp = pred_layer.mlp(pred_norm2)
                        trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_00_detail/mlp", pred_mlp)
                        pred_hidden = pred_residual + pred_mlp
                    else:
                        pred_hidden = pred_layer(pred_hidden, pred_position_embeddings, pred_mask, code_cache[pred_layer_index])
                    trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_layer_{pred_layer_index:02d}", pred_hidden)
                pred_hidden = predictor_model.norm(pred_hidden)
                trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_norm", pred_hidden)
                code_logits = predictor.lm_head[code_idx](pred_hidden)
                trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_logits", code_logits)
                trace_topk(trace, f"generation/step_{step:04d}/predictor_{code_idx:02d}_topk", code_logits[:, -1, :], args.trace_topk)
                next_code = model._sample_token(code_logits, temperature=args.temperature, top_k=args.top_k, top_p=args.top_p)
                mx.eval(next_code)
                trace.tensor(f"generation/step_{step:04d}/predictor_{code_idx:02d}_code", next_code)
                trace.ids(
                    f"generation/step_{step:04d}/predictor_{code_idx:02d}_code_ids",
                    np.array(next_code).reshape(-1).astype(int).tolist(),
                )
                code_tokens.append(next_code)

            all_codes = mx.concatenate(code_tokens, axis=1)
            mx.eval(all_codes)
            trace.tensor(f"generation/step_{step:04d}/all_codes", all_codes)
            all_code_ids = np.array(all_codes).reshape(-1).astype(int).tolist()
            trace.ids(f"generation/step_{step:04d}/all_code_ids", all_code_ids)
            generated_code_frames.append(all_code_ids)

            if trailing_idx < trailing_text_hidden.shape[1]:
                text_embed = trailing_text_hidden[:, trailing_idx : trailing_idx + 1, :]
                trailing_idx += 1
            else:
                text_embed = tts_pad_embed

            codec_embed = model.talker.get_input_embeddings()(next_token)
            for i, code in enumerate(code_tokens[1:]):
                codec_embed = codec_embed + model.talker.code_predictor.codec_embedding[i](code)
            trace.tensor(f"generation/step_{step:04d}/next_codec_embed", codec_embed)
            input_embeds = text_embed + codec_embed
            mx.eval(input_embeds)
            trace.tensor(f"generation/step_{step:04d}/next_input_embeds", input_embeds)

            generated_token_ids.append(int(np.array(next_token)[0, 0]))
            generated_codes.append(all_codes)

        trace.event("generation/generated_code_0_ids", kind="ids", len=len(generated_token_ids), values=generated_token_ids)
        (trace.trace_dir / "generated_codes.json").write_text(json.dumps(generated_code_frames), encoding="utf8")
        if generated_codes:
            gen_codes = mx.stack(generated_codes, axis=1)
            trace.tensor("decode/gen_codes", gen_codes)
            ref_codes_t = mx.transpose(ref_codes, (0, 2, 1))
            full_codes = mx.concatenate([ref_codes_t, gen_codes], axis=1)
            trace.tensor("decode/ref_codes_t", ref_codes_t)
            trace.tensor("decode/full_codes", full_codes)
            audio, audio_lengths = model.speech_tokenizer.decode(full_codes)
            trace.tensor("decode/audio_raw", audio)
            trace.tensor("decode/audio_lengths", audio_lengths)
            audio = audio[0]
            valid_len = int(np.array(audio_lengths)[0])
            if valid_len > 0 and valid_len < audio.shape[0]:
                audio = audio[:valid_len]
            ref_len = ref_codes.shape[2]
            total_len = full_codes.shape[1]
            cut = int(ref_len / max(total_len, 1) * audio.shape[0])
            trace.event("decode/cut", kind="metadata", ref_len=int(ref_len), total_len=int(total_len), valid_len=int(valid_len), cut=int(cut))
            if cut > 0 and cut < audio.shape[0]:
                audio = audio[cut:]
            mx.eval(audio)
            trace.tensor("decode/audio_final", audio)
    finally:
        trace.close()


def main() -> None:
    parser = argparse.ArgumentParser(description="Trace the Python MLX Qwen3-TTS ICL pipeline for Rust parity.")
    parser.add_argument("--model-name", default="mlx-community/Qwen3-TTS-12Hz-1.7B-Base-6bit")
    parser.add_argument("--text", default="Hallo Dino.")
    parser.add_argument("--ref-audio", default="../../data/voices/elevenlabs-pibot-reference-de.wav")
    parser.add_argument("--ref-text-file", default="../../data/voices/elevenlabs-pibot-reference-de.txt")
    parser.add_argument("--ref-text", default=None)
    parser.add_argument("--language", default="de")
    parser.add_argument("--trace-dir", required=True)
    parser.add_argument("--max-steps", type=int, default=4)
    parser.add_argument("--temperature", type=float, default=0.0)
    parser.add_argument("--top-k", type=int, default=0)
    parser.add_argument("--top-p", type=float, default=1.0)
    parser.add_argument("--repetition-penalty", type=float, default=1.5)
    parser.add_argument("--seed", type=int, default=1234)
    parser.add_argument("--trace-topk", type=int, default=8)
    parser.add_argument("--sample-count", type=int, default=8)
    parser.add_argument("--save-tensors", action="store_true")
    args = parser.parse_args()
    args.language = normalize_language(args.language)
    if args.ref_text is None:
        args.ref_text = Path(args.ref_text_file).expanduser().read_text(encoding="utf8").strip()
    run(args)


if __name__ == "__main__":
    main()
