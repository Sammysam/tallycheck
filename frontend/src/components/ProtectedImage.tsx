import { useEffect, useState } from 'react';
import api from '../api/client';

interface ProtectedImageProps {
  /** API path, e.g. `/employees/123/photo`. Fetched with the auth header. */
  src: string | null;
  alt: string;
  className?: string;
  /** Shown while loading or when there is no image — usually initials. */
  fallback: string;
  /** Bump to refetch after an upload. */
  version?: number;
}

/**
 * Employee photos live behind the API (and the login token), so a plain
 * <img src> cannot load them. This fetches the bytes with axios and shows
 * them through an object URL instead.
 */
export default function ProtectedImage({ src, alt, className = '', fallback, version = 0 }: ProtectedImageProps) {
  // Remember which request the URL belongs to, so a stale one is never shown
  // for a new src/version while the fresh fetch is still in flight.
  const [loaded, setLoaded] = useState<{ src: string; version: number; url: string } | null>(null);
  const url = loaded && loaded.src === src && loaded.version === version ? loaded.url : null;

  useEffect(() => {
    if (!src) return;
    let objectUrl: string | null = null;
    let cancelled = false;

    api
      .get(src, { responseType: 'blob' })
      .then((r) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(r.data);
        setLoaded({ src, version, url: objectUrl });
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [src, version]);

  if (!url) {
    return (
      <div
        className={`flex items-center justify-center bg-gradient-to-br from-blue-400 to-indigo-500 text-white font-semibold select-none ${className}`}
        aria-label={alt}
      >
        {fallback}
      </div>
    );
  }

  return <img src={url} alt={alt} className={`object-cover ${className}`} />;
}
