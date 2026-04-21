import React, { useState, useRef, useEffect } from "react";
import { motion } from "motion/react";
import { Play, Pause, Flame, FlameKindling } from "lucide-react";

export default function AudioPlayer({ src, title = "Audio" }) {
  const [isPlaying, setIsPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [duration, setDuration] = useState(0);
  const [isMuted, setIsMuted] = useState(false);
  const [isDocked, setIsDocked] = useState(false);

  const audioRef = useRef(null);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;

    const updateProgress = () => setProgress(audio.currentTime);

    const setAudioDuration = () => {
      if (audio.duration && audio.duration !== Infinity) {
        setDuration(audio.duration);
      }
    };

    const handleEnded = () => {
      setIsPlaying(false);
      setProgress(0);
    };

    if (audio.readyState >= 1) {
      setAudioDuration();
    }

    audio.addEventListener("timeupdate", updateProgress);
    audio.addEventListener("loadedmetadata", setAudioDuration);
    audio.addEventListener("durationchange", setAudioDuration);
    audio.addEventListener("ended", handleEnded);

    return () => {
      audio.removeEventListener("timeupdate", updateProgress);
      audio.removeEventListener("loadedmetadata", setAudioDuration);
      audio.removeEventListener("durationchange", setAudioDuration);
      audio.removeEventListener("ended", handleEnded);
    };
  }, []);

  // Docking Logic
  useEffect(() => {
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (
          !entry.isIntersecting &&
          entry.boundingClientRect.top < 0 &&
          (isPlaying || progress > 0)
        ) {
          setIsDocked(true);
        } else {
          setIsDocked(false);
        }
      },
      { threshold: 0 },
    );

    const wrapper = document.getElementById("audio-player-wrapper");
    if (wrapper) {
      observer.observe(wrapper);
    }

    return () => observer.disconnect();
  }, [isPlaying, progress]);

  const togglePlay = () => {
    if (audioRef.current) {
      if (isPlaying) {
        audioRef.current.pause();
      } else {
        audioRef.current.play();
      }
      setIsPlaying(!isPlaying);
    }
  };

  const toggleMute = () => {
    if (audioRef.current) {
      audioRef.current.muted = !isMuted;
      setIsMuted(!isMuted);
    }
  };

  const handleSeek = (e) => {
    const newTime = Number(e.target.value);
    if (audioRef.current) {
      audioRef.current.currentTime = newTime;
      setProgress(newTime);
    }
  };

  const formatTime = (time) => {
    if (isNaN(time)) return "0:00";
    const minutes = Math.floor(time / 60);
    const seconds = Math.floor(time % 60);
    return `${minutes}:${seconds.toString().padStart(2, "0")}`;
  };

  return (
    <>
      <audio ref={audioRef} src={src} preload="metadata" />

      <motion.div
        layout
        initial={false}
        animate={
          isDocked
            ? {
                position: "fixed",
                bottom: 24,
                left: "50%",
                x: "-50%",
                width: "95%",
                maxWidth: "600px",
                zIndex: 50,
              }
            : {
                position: "relative",
                bottom: "auto",
                left: "auto",
                x: "0%",
                width: "100%",
                maxWidth: "100%",
                zIndex: 10,
              }
        }
        transition={{ type: "spring", stiffness: 200, damping: 25 }}
        className="group"
      >
        <div
          className="relative rounded-xl border border-primary/20 p-5 overflow-hidden transition-shadow duration-300
                     bg-background bg-gradient-to-br from-muted to-background
                     shadow-[0_0_40px_rgba(196,154,43,0.04)]
                     hover:shadow-[0_0_50px_rgba(196,154,43,0.08)]"
        >
          {/* Top glow line */}
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-3/5 h-px bg-gradient-to-r from-transparent via-primary/25 to-transparent" />

          {/* Status line */}
          <div className="mb-3">
            <span className="font-display text-[10px] text-primary tracking-[0.15em]">
              &#9670; {isPlaying ? "NOW PLAYING" : "READY"} &#9670;
            </span>
          </div>

          {/* Controls row */}
          <div className="flex items-center gap-4">
            {/* Play/Pause - circular seal button */}
            <button
              onClick={togglePlay}
              aria-label={isPlaying ? "Pause audio" : "Play audio"}
              className="w-11 h-11 rounded-full border border-primary/30 bg-primary/5
                         hover:bg-primary/10 hover:border-primary/50
                         active:scale-95 transition-all duration-300
                         flex items-center justify-center shrink-0
                         focus-visible:ring-2 focus-visible:ring-primary outline-none"
            >
              {isPlaying ? (
                <Pause className="w-4 h-4 text-primary fill-current" />
              ) : (
                <Play className="w-4 h-4 text-primary fill-current ml-0.5" />
              )}
            </button>

            {/* Title + progress */}
            <div className="flex-1 min-w-0">
              <div className="flex justify-between items-baseline mb-2">
                <h4 className="font-display text-sm text-foreground truncate">{title}</h4>
                <span className="font-body text-xs text-muted-foreground tabular-nums shrink-0 ml-3">
                  {formatTime(progress)}
                  <span className="text-muted-foreground/40 px-1">/</span>
                  {formatTime(duration)}
                </span>
              </div>

              {/* Progress bar */}
              <div className="relative h-[3px] bg-primary/10 rounded-full overflow-hidden">
                <div
                  className="absolute inset-y-0 left-0 bg-gradient-to-r from-primary to-primary/80 rounded-full transition-[width] duration-100 ease-linear"
                  style={{ width: `${duration ? (progress / duration) * 100 : 0}%` }}
                />
                <input
                  type="range"
                  min="0"
                  max={duration || 100}
                  value={progress}
                  onChange={handleSeek}
                  aria-label="Seek slider"
                  className="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
                />
              </div>
            </div>

            {/* Flame volume toggle */}
            <button
              onClick={toggleMute}
              aria-label={isMuted ? "Unmute" : "Mute"}
              className="shrink-0 p-2 rounded-full transition-colors duration-300
                         hover:bg-primary/10
                         focus-visible:ring-2 focus-visible:ring-primary outline-none"
            >
              {isMuted ? (
                <FlameKindling className="w-4 h-4 text-muted-foreground/50" />
              ) : (
                <Flame className="w-4 h-4 text-primary" />
              )}
            </button>
          </div>

          {/* Bottom glow line */}
          <div className="absolute bottom-0 left-1/2 -translate-x-1/2 w-3/5 h-px bg-gradient-to-r from-transparent via-primary/15 to-transparent" />
        </div>
      </motion.div>
    </>
  );
}
