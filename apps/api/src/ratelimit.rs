use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

/// Limiteur de débit en mémoire, fenêtre glissante par IP (anti force brute
/// sur /auth/login). Suffisant pour un outil interne mono-instance : derrière
/// le BFF Next.js, l'IP réelle du client est lue via X-Forwarded-For.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
    max: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max,
            window,
        }
    }

    /// Enregistre une tentative. Renvoie Err(()) si la limite est atteinte
    /// sur la fenêtre glissante.
    pub async fn hit(&self, ip: IpAddr) -> Result<(), ()> {
        let mut map = self.inner.lock().await;
        // Borne la mémoire : purge globale quand la table grossit trop.
        if map.len() > 10_000 {
            let now = Instant::now();
            map.retain(|_, v| {
                v.last()
                    .is_some_and(|t| now.duration_since(*t) < self.window)
            });
        }
        let now = Instant::now();
        let entry = map.entry(ip).or_default();
        entry.retain(|t| now.duration_since(*t) < self.window);
        if entry.len() >= self.max {
            return Err(());
        }
        entry.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn limite_atteinte_puis_lebignee_apres_la_fenetre() {
        let rl = RateLimiter::new(3, Duration::from_millis(50));
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert!(rl.hit(ip).await.is_ok());
        assert!(rl.hit(ip).await.is_ok());
        assert!(rl.hit(ip).await.is_ok());
        assert!(rl.hit(ip).await.is_err());
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(rl.hit(ip).await.is_ok());
    }

    #[tokio::test]
    async fn ips_independantes() {
        let rl = RateLimiter::new(1, Duration::from_secs(60));
        let a = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        assert!(rl.hit(a).await.is_ok());
        assert!(rl.hit(a).await.is_err());
        assert!(rl.hit(b).await.is_ok());
    }
}
