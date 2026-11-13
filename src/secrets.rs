use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};
use aes::Aes128;
use base64::{engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD}, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use rand::RngCore;
use sha2::Sha256;
use crate::{paths::truncate_utf8, AppError, Result};

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;
type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct SecretStore {
    directory: PathBuf,
    key: [u8; 32],
    cache: std::sync::Arc<Mutex<BTreeMap<String, String>>>,
}

impl SecretStore {
    pub fn new(directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;
        let path = directory.join(".secret-key");
        let encoded = if path.exists() {
            fs::read_to_string(path)?
        } else {
            let mut key = [0u8; 32];
            rand::rng().fill_bytes(&mut key);
            let encoded = URL_SAFE.encode(key);
            fs::write(&path, &encoded)?;
            encoded
        };
        let key: [u8; 32] = URL_SAFE.decode(encoded.trim()).map_err(|_|AppError::Other("本地密钥格式无效".into()))?.try_into().map_err(|_|AppError::Other("本地密钥长度无效".into()))?;
        Ok(Self { directory, key, cache: Default::default() })
    }

    fn all(&self) -> BTreeMap<String, String> {
        let path = self.directory.join("secrets.json");
        if !path.exists() { return Default::default(); }
        serde_json::from_str(&fs::read_to_string(path).unwrap_or_default()).unwrap_or_default()
    }

    pub fn set(&self, name: &str, value: &str) -> Result<()> {
        if name.is_empty() || !name.chars().all(|c|c.is_ascii_uppercase()||c.is_ascii_digit()||c=='_') || !name.chars().next().unwrap().is_ascii_uppercase() || name.len()>81 {
            return Err(AppError::Invalid("密钥变量名不合法".into()));
        }
        let mut all=self.all();
        all.insert(name.into(),self.encrypt(value)?);
        let path=self.directory.join("secrets.json");
        let temp=self.directory.join("secrets.json.tmp");
        fs::write(&temp,serde_json::to_string(&all)?)?;
        fs::rename(temp,path)?;
        self.cache.lock().insert(name.into(),value.into());
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<String>> {
        if let Some(value)=self.cache.lock().get(name) { return Ok(Some(value.clone())); }
        if let Some(value)=self.all().get(name) {
            let value=self.decrypt(value)?;
            self.cache.lock().insert(name.into(),value.clone());
            return Ok(Some(value));
        }
        Ok(std::env::var(name).ok())
    }

    pub fn configured(&self, name: &str) -> bool { self.all().contains_key(name) || std::env::var_os(name).is_some() }

    pub fn redact(&self, text: &str) -> String {
        let mut result=text.to_string();
        for name in self.all().keys().cloned().chain(["DEEPSEEK_API_KEY".to_string(),"WEB_SEARCH_API_KEY".to_string()]).collect::<Vec<_>>() {
            if let Ok(Some(secret))=self.get(&name) {
                if secret.len()>=6 { result=result.replace(&secret,"[已隐藏]"); }
            }
        }
        truncate_utf8(&result,4096).to_string()
    }

    fn encrypt(&self, value: &str) -> Result<String> {
        let mut iv=[0u8;16]; rand::rng().fill_bytes(&mut iv);
        let mut aes_key=[0u8;16]; aes_key.copy_from_slice(&self.key[..16]);
        let encrypted=Aes128CbcEnc::new(&aes_key.into(),&iv.into()).encrypt_padded_vec_mut::<Pkcs7>(value.as_bytes());
        let timestamp=chrono::Utc::now().timestamp() as u64;
        let mut body=vec![0x80]; body.extend_from_slice(&timestamp.to_be_bytes()); body.extend_from_slice(&iv); body.extend_from_slice(&encrypted);
        let mut mac=<HmacSha256 as Mac>::new_from_slice(&self.key[16..]).map_err(|_|AppError::Other("无法创建密钥校验".into()))?;
        mac.update(&body); body.extend_from_slice(&mac.finalize().into_bytes());
        Ok(URL_SAFE_NO_PAD.encode(body))
    }

    fn decrypt(&self, token: &str) -> Result<String> {
        let raw=URL_SAFE_NO_PAD.decode(token).or_else(|_|URL_SAFE.decode(token)).map_err(|_|AppError::Other("本地密钥无法解码，请重新配置".into()))?;
        if raw.len()<1+8+16+32 || raw[0]!=0x80 { return Err(AppError::Other("本地密钥格式不受支持".into())); }
        let split=raw.len()-32;
        let mut mac=<HmacSha256 as Mac>::new_from_slice(&self.key[16..]).map_err(|_|AppError::Other("无法创建密钥校验".into()))?;
        mac.update(&raw[..split]);
        mac.verify_slice(&raw[split..]).map_err(|_|AppError::Other("本地密钥校验失败，请重新配置".into()))?;
        let mut aes_key=[0u8;16]; aes_key.copy_from_slice(&self.key[..16]);
        let mut iv=[0u8;16]; iv.copy_from_slice(&raw[9..25]);
        let plaintext=Aes128CbcDec::new(&aes_key.into(),&iv.into()).decrypt_padded_vec_mut::<Pkcs7>(&raw[25..split]).map_err(|_|AppError::Other("本地密钥无法解密，请重新配置".into()))?;
        String::from_utf8(plaintext).map_err(|_|AppError::Other("本地密钥不是有效 UTF-8".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypts_and_reads_without_plaintext() {
        let dir=tempfile::tempdir().unwrap();
        let store=SecretStore::new(dir.path()).unwrap();
        store.set("TEST_KEY","secret-value").unwrap();
        assert_eq!(store.get("TEST_KEY").unwrap().unwrap(),"secret-value");
        assert!(!std::fs::read_to_string(dir.path().join("secrets.json")).unwrap().contains("secret-value"));
        assert_eq!(store.redact("secret-value"),"[已隐藏]");
    }
}
