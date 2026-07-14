use std::path::{Path, PathBuf};

/// Recebe o diretório root do usuário e o subcaminho solicitado.
/// Retorna o caminho completo e canonicalizado no host físico, ou erro se for fora do root.
pub fn validate_and_resolve_path(root: &str, requested: &str) -> Result<PathBuf, String> {
    let root_path = Path::new(root);
    
    // Decodifica url se necessário e remove barra inicial
    let clean_requested = requested.trim_start_matches('/');
    
    // Cria o caminho de destino teórico
    let target = root_path.join(clean_requested);
    
    // Simplifica e canonicaliza o caminho de forma léxica (sem exigir que o arquivo/pasta exista no host físico)
    let clean_target = clean_lexically(&target);

    // O caminho final gerado DEVE começar com o root_path
    if clean_target.starts_with(root_path) {
        Ok(clean_target)
    } else {
        Err("Acesso negado: Tentativa de Directory Traversal detectada".to_string())
    }
}

// Uma implementação leve de simplificação lexical sem I/O
fn clean_lexically(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::Normal(c) => {
                components.push(c);
            }
            std::path::Component::RootDir => {
                // Mantém root do path absoluto
            }
            std::path::Component::Prefix(_) => {
                // Trata prefixes em sistemas como Windows
            }
            std::path::Component::CurDir => {}
        }
    }
    
    let mut result = PathBuf::new();
    if path.is_absolute() {
        // No Unix/Linux adiciona a barra '/'
        result.push(std::path::Component::RootDir.as_os_str());
    }
    for c in components {
        result.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_paths() {
        let root = "/data/user1";
        assert!(validate_and_resolve_path(root, "pasta/arquivo.txt").is_ok());
        assert_eq!(
            validate_and_resolve_path(root, "fotos/../pasta/file.png").unwrap(),
            PathBuf::from("/data/user1/pasta/file.png")
        );
    }

    #[test]
    fn test_traversal_prevention() {
        let root = "/data/user1";
        assert!(validate_and_resolve_path(root, "../../etc/passwd").is_err());
        assert!(validate_and_resolve_path(root, "fotos/../../../etc/shadow").is_err());
        assert!(validate_and_resolve_path(root, "..").is_err());
    }
}
