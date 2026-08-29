use semver::Version;
use toml::Value as TomlValue;

/// Machine-readable channel name derived from the updater's local version cache.
///
/// Reads `stable_version` from `~/.grow/version.json` (written by the
/// auto-updater) and compares the compiled-in version against it:
/// - `Some("alpha")` when the current version is ahead of stable,
/// - `Some("stable")` when at or behind stable,
/// - `None` when no cached pointer is available (first launch, old cache).
///
/// This is a lightweight duplicate of `update::channel_name()` for
/// use in `shell` which cannot depend on `update`.
pub fn channel_name_from_cache() -> Option<&'static str> {
    use std::sync::OnceLock;
    static NAME: OnceLock<Option<&'static str>> = OnceLock::new();
    *NAME.get_or_init(|| {
        let version_path = crate::util::grow_home::grow_home().join("version.json");
        let content = std::fs::read_to_string(&version_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let stable = parsed.get("stable_version")?.as_str()?;
        let current = semver::Version::parse(version::VERSION).ok()?;
        let stable_v = semver::Version::parse(stable).ok()?;
        if current > stable_v {
            Some("alpha")
        } else {
            Some("stable")
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VersionKnob {
    Minimum,
    Maximum,
    RequiredMinimum,
    RequiredMaximum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    Floor,
    Ceiling,
}

impl VersionKnob {
    pub(crate) fn toml_key(self) -> &'static str {
        match self {
            VersionKnob::Minimum => "minimum_version",
            VersionKnob::Maximum => "maximum_version",
            VersionKnob::RequiredMinimum => "required_minimum_version",
            VersionKnob::RequiredMaximum => "required_maximum_version",
        }
    }

    pub(crate) fn env_var(self) -> &'static str {
        match self {
            VersionKnob::Minimum => "GROW_MINIMUM_VERSION",
            VersionKnob::Maximum => "GROW_MAXIMUM_VERSION",
            VersionKnob::RequiredMinimum => "GROW_REQUIRED_MINIMUM_VERSION",
            VersionKnob::RequiredMaximum => "GROW_REQUIRED_MAXIMUM_VERSION",
        }
    }

    fn bound(self) -> Bound {
        match self {
            VersionKnob::Minimum | VersionKnob::RequiredMinimum => Bound::Floor,
            VersionKnob::Maximum | VersionKnob::RequiredMaximum => Bound::Ceiling,
        }
    }
}

fn cli_version_from_toml(root: &TomlValue, key: &str) -> Option<String> {
    root.get("cli")?.get(key)?.as_str().map(str::to_owned)
}

fn env_version(var: &str) -> Option<String> {
    std::env::var(var).ok()
}

/// `cli.<key>` from the local config layer.
fn version_candidates(layers: &crate::config::ConfigLayers, key: &str) -> Vec<String> {
    cli_version_from_toml(&layers.user, key)
        .into_iter()
        .collect()
}

fn fold_bound(raws: Vec<String>, knob: VersionKnob) -> Option<Version> {
    let mut best: Option<Version> = None;
    for raw in raws {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match Version::parse(trimmed) {
            Ok(v) => {
                best = Some(match (best, knob.bound()) {
                    (None, _) => v,
                    (Some(cur), Bound::Floor) => cur.max(v),
                    (Some(cur), Bound::Ceiling) => cur.min(v),
                });
            }
            Err(source) => tracing::warn!(
                knob = knob.toml_key(),
                value = %trimmed,
                error = %source,
                "ignoring invalid version bound"
            ),
        }
    }
    best
}

/// Env joins the same extreme as the local config, so it can only tighten a bound.
fn resolve_version_bound<E: Fn(&str) -> Option<String>>(
    layers: &crate::config::ConfigLayers,
    env: &E,
    knob: VersionKnob,
) -> Option<Version> {
    let mut raws = version_candidates(layers, knob.toml_key());
    raws.extend(env(knob.env_var()));
    fold_bound(raws, knob)
}

/// The four resolved version bounds: soft `minimum`/`maximum` steer the updater;
/// hard `required_*` gate startup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VersionPolicy {
    pub minimum: Option<Version>,
    pub maximum: Option<Version>,
    pub required_minimum: Option<Version>,
    pub required_maximum: Option<Version>,
}

impl VersionPolicy {
    /// Resolve from config layers and env; every knob fails open.
    pub fn resolve() -> Self {
        let layers = crate::config::ConfigLayers::load().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "version policy: config layers failed to load; using env overrides only");
            crate::config::ConfigLayers::default()
        });
        Self::from_layers(&layers, &env_version)
    }

    fn from_layers<E: Fn(&str) -> Option<String>>(
        layers: &crate::config::ConfigLayers,
        env: &E,
    ) -> Self {
        let get = |knob| resolve_version_bound(layers, env, knob);
        let minimum = get(VersionKnob::Minimum);
        let maximum = get(VersionKnob::Maximum);
        let required_minimum = get(VersionKnob::RequiredMinimum);
        let required_maximum = get(VersionKnob::RequiredMaximum);

        // A contradictory required range is retained and handled as fail-open by
        // `effective_required_*`.
        if let (Some(lo), Some(hi)) = (&required_minimum, &required_maximum)
            && lo > hi
        {
            tracing::warn!(%lo, %hi, "required_minimum_version exceeds required_maximum_version; requirement will be ignored");
        }

        if let (Some(lo), Some(hi)) = (&minimum, &maximum)
            && lo > hi
        {
            tracing::warn!(%lo, %hi, "minimum_version exceeds maximum_version; updates will be skipped");
        }

        Self {
            minimum,
            maximum,
            required_minimum,
            required_maximum,
        }
    }

    /// An unsatisfiable required range is ignored (fail-open).
    pub fn has_contradictory_required_range(&self) -> bool {
        matches!(
            (&self.required_minimum, &self.required_maximum),
            (Some(lo), Some(hi)) if lo > hi
        )
    }

    /// `None` on a contradictory range, so the fail-open guard lives in one place.
    fn effective_required_minimum(&self) -> Option<&Version> {
        (!self.has_contradictory_required_range())
            .then_some(self.required_minimum.as_ref())
            .flatten()
    }

    fn effective_required_maximum(&self) -> Option<&Version> {
        (!self.has_contradictory_required_range())
            .then_some(self.required_maximum.as_ref())
            .flatten()
    }

    /// Shared clamp core: cap at the ceilings, then the hard `required_minimum`
    /// last so it wins over a lower ceiling.
    fn clamp_version(&self, mut v: Version) -> Version {
        if let Some(c) = &self.maximum
            && v > *c
        {
            v = c.clone();
        }
        if let Some(hi) = self.effective_required_maximum()
            && v > *hi
        {
            v = hi.clone();
        }
        if let Some(lo) = self.effective_required_minimum()
            && v < *lo
        {
            v = lo.clone();
        }
        v
    }

    /// Clamp then skip; the single place that ordering lives. `None` means an
    /// anti-downgrade skip.
    pub fn resolve_target(&self, latest: &str) -> Option<String> {
        let target = self.clamp(latest);
        (!self.skips_update_target(&target)).then_some(target)
    }

    /// Clamp `target` into range. An unparseable target resolves to the lowest
    /// in-range version when a hard floor applies, else passes through unchanged.
    fn clamp(&self, target: &str) -> String {
        match Version::parse(target) {
            Ok(v) => self.clamp_version(v).to_string(),
            Err(_) if self.effective_required_minimum().is_some() => {
                self.clamp_version(Version::new(0, 0, 0)).to_string()
            }
            Err(_) => target.to_string(),
        }
    }

    /// Anti-downgrade: skip a target below the soft `minimum`. Never clamps up.
    fn skips_update_target(&self, target: &str) -> bool {
        matches!(
            (&self.minimum, Version::parse(target)),
            (Some(min), Ok(t)) if t < *min
        )
    }

    /// Lowest version an explicit `--version` pin may install, always agreeing
    /// with [`clamp`](Self::clamp). Only the hard `required_minimum` blocks a pin.
    pub fn installable_floor(&self) -> Option<Version> {
        self.effective_required_minimum()?;
        Some(self.clamp_version(Version::new(0, 0, 0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn layers(user: &str) -> crate::config::ConfigLayers {
        let parse = |s: &str| {
            if s.is_empty() {
                TomlValue::Table(Default::default())
            } else {
                toml::from_str(s).unwrap()
            }
        };
        crate::config::ConfigLayers {
            user: parse(user),
            ..Default::default()
        }
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn floor_is_semver_max_ceiling_is_semver_min_across_layers() {
        let l = layers("[cli]\nminimum_version = \"0.1.50\"\nmaximum_version = \"0.2.130\"\n");
        let p = VersionPolicy::from_layers(&l, &no_env);
        assert_eq!(p.minimum, Some(v("0.1.50")));
        assert_eq!(p.maximum, Some(v("0.2.130")));
    }

    #[test]
    fn env_tightens_but_cannot_loosen() {
        let l = layers("[cli]\nminimum_version = \"0.2.100\"\nmaximum_version = \"0.2.200\"\n");
        let tighten = |var: &str| match var {
            "GROW_MINIMUM_VERSION" => Some("0.2.150".to_string()),
            "GROW_MAXIMUM_VERSION" => Some("0.2.180".to_string()),
            _ => None,
        };
        let p = VersionPolicy::from_layers(&l, &tighten);
        assert_eq!(p.minimum, Some(v("0.2.150")));
        assert_eq!(p.maximum, Some(v("0.2.180")));

        let loosen = |var: &str| match var {
            "GROW_MINIMUM_VERSION" => Some("0.2.1".to_string()),
            "GROW_MAXIMUM_VERSION" => Some("0.2.999".to_string()),
            _ => None,
        };
        let p = VersionPolicy::from_layers(&l, &loosen);
        assert_eq!(p.minimum, Some(v("0.2.100")));
        assert_eq!(p.maximum, Some(v("0.2.200")));
    }

    #[test]
    fn every_knob_fails_open_on_an_invalid_value() {
        let l = layers(
            "[cli]\nminimum_version = \"nope\"\nmaximum_version = \"bad\"\n\
             required_minimum_version = \"junk\"\nrequired_maximum_version = \"0.2.150\"\n",
        );
        let p = VersionPolicy::from_layers(&l, &no_env);
        assert_eq!(p.minimum, None);
        assert_eq!(p.maximum, None);
        assert_eq!(p.required_minimum, None);
        assert_eq!(p.required_maximum, Some(v("0.2.150")));
    }

    fn pol(
        min: Option<&str>,
        max: Option<&str>,
        rmin: Option<&str>,
        rmax: Option<&str>,
    ) -> VersionPolicy {
        VersionPolicy {
            minimum: min.map(v),
            maximum: max.map(v),
            required_minimum: rmin.map(v),
            required_maximum: rmax.map(v),
        }
    }

    #[test]
    fn soft_minimum_skips_a_downgrade_but_never_clamps_up() {
        let p = pol(Some("0.2.100"), None, None, None);
        assert!(p.skips_update_target("0.2.50"));
        assert_eq!(p.clamp("0.2.50"), "0.2.50");
        assert!(!p.skips_update_target("0.2.100"));
        assert!(!p.skips_update_target("dev"));
        assert!(!pol(None, None, None, None).skips_update_target("0.0.1"));
        assert_eq!(p.installable_floor(), None);
    }

    #[test]
    fn clamp_caps_at_ceilings_and_the_hard_floor_wins() {
        assert_eq!(pol(None, None, None, None).clamp("0.2.200"), "0.2.200");
        assert_eq!(
            pol(None, Some("0.2.150"), None, None).clamp("0.2.200"),
            "0.2.150"
        );
        assert_eq!(
            pol(None, None, None, Some("0.2.150")).clamp("0.2.200"),
            "0.2.150"
        );
        // Hard floor wins over a lower soft ceiling.
        assert_eq!(
            pol(None, Some("0.2.100"), Some("0.2.180"), None).clamp("0.2.50"),
            "0.2.180"
        );
        // Contradictory hard range is ignored (fail open).
        assert_eq!(
            pol(None, None, Some("0.3.0"), Some("0.2.0")).clamp("0.2.120"),
            "0.2.120"
        );
        // Unparseable target: floored to the hard minimum, else passed through.
        assert_eq!(
            pol(None, None, Some("0.2.100"), None).clamp("dev"),
            "0.2.100"
        );
        assert_eq!(pol(None, None, None, None).clamp("dev"), "dev");
    }

    #[test]
    fn resolve_target_clamps_then_skips() {
        assert_eq!(
            pol(None, None, None, None).resolve_target("0.2.200"),
            Some("0.2.200".into())
        );
        assert_eq!(
            pol(Some("0.2.100"), None, None, None).resolve_target("0.2.50"),
            None
        );
        assert_eq!(
            pol(None, Some("0.2.150"), None, None).resolve_target("0.2.200"),
            Some("0.2.150".into())
        );
        // max < min clamps below the floor, then the skip catches the clamped
        // value. This is the ordering every updater path depends on.
        assert_eq!(
            pol(Some("0.2.100"), Some("0.2.50"), None, None).resolve_target("0.2.200"),
            None
        );
    }

    #[test]
    fn installable_floor_tracks_only_the_hard_minimum() {
        assert_eq!(
            pol(None, None, Some("0.2.120"), None).installable_floor(),
            Some(v("0.2.120"))
        );
        // Contradictory hard range is ignored, so there is no floor.
        assert_eq!(
            pol(None, None, Some("0.3.0"), Some("0.2.0")).installable_floor(),
            None
        );
    }

    #[test]
    fn whitespace_and_empty_values_are_ignored() {
        let l = layers("[cli]\nminimum_version = \"   \"\n");
        let p = VersionPolicy::from_layers(&l, &no_env);
        assert_eq!(p.minimum, None);
    }
}
