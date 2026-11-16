//! Génération pure des commandes de provisioning par plateforme
//! (VyOS, Cisco IOS, MikroTik RouterOS). Testable sans aucun appareil.

use ipnetwork::Ipv4Network;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Vyos,
    Cisco,
    Mikrotik,
}

impl Platform {
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "vyos" => Some(Platform::Vyos),
            "cisco" => Some(Platform::Cisco),
            "mikrotik" => Some(Platform::Mikrotik),
            _ => None,
        }
    }
}

/// Nom d'interface/description sûr (MikroTik n'aime pas les espaces ni quotes).
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Commandes de création d'un VLAN.
pub fn vlan_commands(p: Platform, vid: u16, name: &str, uplink: &str) -> Vec<String> {
    let name = sanitize(name);
    match p {
        Platform::Vyos => vec![
            format!("set vlans vlan{vid} id '{vid}'"),
            format!("set vlans vlan{vid} description '{name}'"),
        ],
        Platform::Cisco => vec![format!("vlan {vid}"), format!("name {name}")],
        Platform::Mikrotik => vec![format!(
            "/interface vlan add name={name}-{vid} vlan-id={vid} interface={uplink}"
        )],
    }
}

/// Commandes d'adressage d'un sous-réseau sur un VLAN (SVI / vif / interface vlan).
pub fn subnet_commands(
    p: Platform,
    cidr: &str,
    vid: u16,
    vlan_name: &str,
    uplink: &str,
) -> Result<Vec<String>, String> {
    let name = sanitize(vlan_name);
    match p {
        Platform::Vyos => Ok(vec![format!(
            "set interfaces ethernet {uplink} vif {vid} address '{cidr}'"
        )]),
        Platform::Cisco => {
            let net: Ipv4Network = cidr
                .parse()
                .map_err(|e| format!("CIDR invalide ({cidr}): {e}"))?;
            Ok(vec![
                format!("interface Vlan{vid}"),
                format!("ip address {} {}", net.ip(), net.mask()),
                "no shutdown".to_string(),
            ])
        }
        Platform::Mikrotik => Ok(vec![format!(
            "/ip address add address={cidr} interface={name}-{vid}"
        )]),
    }
}

/// Enveloppe de session pour un exec SSH multi-lignes.
pub fn wrap_session(p: Platform, commands: &[String]) -> String {
    let body = commands.join("\n");
    match p {
        Platform::Cisco => format!("configure terminal\n{body}\nend\nwrite memory"),
        Platform::Vyos => format!("configure\n{body}\ncommit\nsave\nexit"),
        Platform::Mikrotik => body,
    }
}

/// Convertit une commande CLI MikroTik en mots d'API RouterOS.
/// "/interface vlan add name=x vlan-id=10" -> ["/interface/vlan/add", "=name=x", "=vlan-id=10"]
pub fn cli_to_api_words(cmd: &str) -> Vec<String> {
    let mut path: Vec<String> = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    for tok in cmd.split_whitespace() {
        if tok.starts_with('=') {
            attrs.push(tok.to_string());
        } else if tok.contains('=') {
            attrs.push(format!("={tok}"));
        } else {
            path.push(tok.trim_start_matches('/').to_string());
        }
    }
    let mut words = vec![format!("/{}", path.join("/"))];
    words.extend(attrs);
    words
}

/// Heuristique de détection d'erreur dans la sortie d'une commande.
pub fn output_has_error(output: &str) -> bool {
    let low = output.to_lowercase();
    [
        "% invalid",
        "% ambiguous",
        "% incomplete",
        "error:",
        "failure:",
        "expected end of command",
        "syntax error",
        "bad command",
        "no such command",
    ]
    .iter()
    .any(|m| low.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlan_vyos_cisco_mikrotik() {
        assert_eq!(
            vlan_commands(Platform::Vyos, 10, "Prod", "eth0"),
            vec![
                "set vlans vlan10 id '10'",
                "set vlans vlan10 description 'Prod'"
            ]
        );
        assert_eq!(
            vlan_commands(Platform::Cisco, 10, "Prod", "eth0"),
            vec!["vlan 10", "name Prod"]
        );
        assert_eq!(
            vlan_commands(Platform::Mikrotik, 10, "Prod serveurs", "ether1"),
            vec!["/interface vlan add name=Prod-serveurs-10 vlan-id=10 interface=ether1"]
        );
    }

    #[test]
    fn subnet_cisco_netmask() {
        let cmds = subnet_commands(Platform::Cisco, "10.0.10.0/24", 10, "Prod", "eth0").unwrap();
        assert_eq!(
            cmds,
            vec![
                "interface Vlan10",
                "ip address 10.0.10.0 255.255.255.0",
                "no shutdown"
            ]
        );
        let cmds = subnet_commands(Platform::Cisco, "192.168.5.0/29", 5, "DMZ", "eth0").unwrap();
        assert_eq!(cmds[1], "ip address 192.168.5.0 255.255.255.248");
        assert!(subnet_commands(Platform::Cisco, "10.0.0.0/33", 5, "DMZ", "eth0").is_err());
    }

    #[test]
    fn subnet_vyos_mikrotik() {
        assert_eq!(
            subnet_commands(Platform::Vyos, "10.0.10.0/24", 10, "Prod", "eth0").unwrap(),
            vec!["set interfaces ethernet eth0 vif 10 address '10.0.10.0/24'"]
        );
        assert_eq!(
            subnet_commands(Platform::Mikrotik, "10.0.10.0/24", 10, "Prod", "ether1").unwrap(),
            vec!["/ip address add address=10.0.10.0/24 interface=Prod-10"]
        );
    }

    #[test]
    fn wrap_sessions() {
        let cmds = vec!["vlan 10".to_string(), "name Prod".to_string()];
        assert_eq!(
            wrap_session(Platform::Cisco, &cmds),
            "configure terminal\nvlan 10\nname Prod\nend\nwrite memory"
        );
        assert_eq!(
            wrap_session(Platform::Vyos, &cmds),
            "configure\nvlan 10\nname Prod\ncommit\nsave\nexit"
        );
        assert_eq!(
            wrap_session(Platform::Mikrotik, &cmds),
            "vlan 10\nname Prod"
        );
    }

    #[test]
    fn cli_vers_api() {
        assert_eq!(
            cli_to_api_words("/interface vlan add name=Prod-10 vlan-id=10 interface=ether1"),
            vec![
                "/interface/vlan/add",
                "=name=Prod-10",
                "=vlan-id=10",
                "=interface=ether1"
            ]
        );
        assert_eq!(
            cli_to_api_words("/ip address add address=10.0.10.0/24 interface=Prod-10"),
            vec![
                "/ip/address/add",
                "=address=10.0.10.0/24",
                "=interface=Prod-10"
            ]
        );
    }

    #[test]
    fn detection_erreurs() {
        assert!(output_has_error("% Invalid input detected at '^'"));
        assert!(output_has_error("failure: already have such address"));
        assert!(!output_has_error(""));
        assert!(!output_has_error("RouterOS 7.15 stable"));
    }
}
