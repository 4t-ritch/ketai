//! Tipos y framing del protocolo IPC de ketai (docs/protocol.md).
//!
//! Este crate no hace I/O: solo convierte lineas en peticiones y respuestas
//! en lineas. Asi la misma logica se puede portar tal cual a C.

use serde_json::{Map, Value, json};

/// Version del protocolo que habla este daemon.
pub const PROTOCOL_VERSION: u64 = 1;

/// Longitud maxima de una linea, sin contar el `\n` final.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Codigos de error de la seccion 8.
pub mod codes {
  pub const PARSE_ERROR: i64 = -32700;
  pub const INVALID_REQUEST: i64 = -32600;
  pub const METHOD_NOT_FOUND: i64 = -32601;
  pub const INVALID_PARAMS: i64 = -32602;
  pub const INTERNAL_ERROR: i64 = -32603;
  pub const PROTOCOL_MISMATCH: i64 = 1000;
}

/// Peticion ya validada.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
  pub id: u64,
  pub method: String,
  /// Siempre es un objeto; si el cliente omitio `params`, queda vacio.
  pub params: Map<String, Value>,
}

/// Error que se devuelve al cliente.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorReply {
  /// `None` cuando no se pudo leer un `id` valido de la peticion.
  pub id: Option<u64>,
  pub code: i64,
  pub message: String,
  /// Detalle opcional para el cliente (`error.data`).
  pub data: Option<Value>,
}

impl ErrorReply {
  pub fn new(id: Option<u64>, code: i64, message: impl Into<String>) -> Self {
    Self { id, code, message: message.into(), data: None }
  }

  pub fn with_data(mut self, data: Value) -> Self {
    self.data = Some(data);
    self
  }
}

/// Convierte una linea (sin el `\n`) en una peticion.
pub fn parse_request(line: &[u8]) -> Result<Request, ErrorReply> {
  let value: Value = serde_json::from_slice(line)
    .map_err(|_| ErrorReply::new(None, codes::PARSE_ERROR, "parse error"))?;

  let Value::Object(mut obj) = value else {
    return Err(ErrorReply::new(None, codes::INVALID_REQUEST, "request must be an object"));
  };

  // El id debe ser un entero sin signo; si no, no se puede responder con el.
  let id = match obj.get("id") {
    Some(Value::Number(n)) => n.as_u64(),
    _ => None,
  }
  .ok_or_else(|| ErrorReply::new(None, codes::INVALID_REQUEST, "invalid or missing id"))?;

  let method = match obj.remove("method") {
    Some(Value::String(m)) => m,
    _ => return Err(ErrorReply::new(Some(id), codes::INVALID_REQUEST, "invalid or missing method")),
  };

  let params = match obj.remove("params") {
    None | Some(Value::Null) => Map::new(),
    Some(Value::Object(p)) => p,
    Some(_) => return Err(ErrorReply::new(Some(id), codes::INVALID_PARAMS, "params must be an object")),
  };

  Ok(Request { id, method, params })
}

/// Linea de respuesta exitosa, terminada en `\n`.
pub fn encode_result(id: u64, result: Value) -> String {
  let mut line = json!({"id": id, "result": result}).to_string();
  line.push('\n');
  line
}

/// Linea de respuesta de error, terminada en `\n`.
pub fn encode_error(err: &ErrorReply) -> String {
  let mut error = json!({"code": err.code, "message": err.message});
  if let Some(data) = &err.data {
    error["data"] = data.clone();
  }
  let mut line = json!({"id": err.id, "error": error}).to_string();
  line.push('\n');
  line
}

/// Linea de evento (sin `id`), terminada en `\n`.
pub fn encode_event(event: &str, data: Value) -> String {
  let mut line = json!({"event": event, "data": data}).to_string();
  line.push('\n');
  line
}

/// Topicos de la seccion 6.
pub const TOPICS: [&str; 7] = ["player", "queue", "playlist", "output", "library", "lyrics", "cache"];

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_request_with_params() {
    let req = parse_request(br#"{"id":1,"method":"player.seek","params":{"position_ms":5}}"#).unwrap();
    assert_eq!(req.id, 1);
    assert_eq!(req.method, "player.seek");
    assert_eq!(req.params["position_ms"], 5);
  }

  #[test]
  fn missing_params_become_empty_object() {
    let req = parse_request(br#"{"id":2,"method":"player.play"}"#).unwrap();
    assert!(req.params.is_empty());
  }

  #[test]
  fn non_object_params_are_invalid() {
    let err = parse_request(br#"{"id":2,"method":"x","params":[1]}"#).unwrap_err();
    assert_eq!((err.id, err.code), (Some(2), codes::INVALID_PARAMS));
  }

  #[test]
  fn non_object_request_is_invalid() {
    let err = parse_request(b"[1,2]").unwrap_err();
    assert_eq!((err.id, err.code), (None, codes::INVALID_REQUEST));
  }

  #[test]
  fn encoded_lines_end_with_newline() {
    assert!(encode_result(1, json!({})).ends_with('\n'));
    let err = ErrorReply::new(None, codes::PARSE_ERROR, "parse error");
    assert_eq!(encode_error(&err), "{\"error\":{\"code\":-32700,\"message\":\"parse error\"},\"id\":null}\n");
  }
}
