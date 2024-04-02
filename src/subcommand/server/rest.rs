use {
  super::*,
  axum::{
    response::Json,
    extract::{Extension, Path},
    extract,
  },
  serde_json::{Value, json, to_string},
  base64::{Engine as _, engine::general_purpose},
  hex,
};


pub(super) struct Rest {

}


impl Rest {

  pub async fn inscription(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(query)): Path<DeserializeFromStr<query::Inscription>>,
  ) -> ServerResult<Json<Value>>{

    Ok(
      Json(_get_inscription(&server_config, &index, query)?)
    )
  }

  pub async fn inscriptions(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(inscription_ids): extract::Json<Vec<DeserializeFromStr<query::Inscription>>>,
  ) -> ServerResult<Json<Vec<Value>>>{
    let mut _inscriptions: Vec<Value> = vec![];

    for inscription_id in inscription_ids {
      let inscription = _get_inscription(&server_config, &index, inscription_id.0)?;
      _inscriptions.push(inscription);
    }

    Ok(
      Json(_inscriptions)
    )
  }

  pub async fn sat(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(DeserializeFromStr(sat)): Path<DeserializeFromStr<Sat>>,
  ) -> ServerResult<Json<Value>> {
    let satpoint = index.rare_sat_satpoint(sat)?;
    Ok(
      Json(json!({
        "sat": sat.n().to_string(),
        "decimal": sat.decimal().to_string(),
        "degree": sat.degree().to_string(),
        "percentile": sat.percentile(),
        "name": sat.name(),
        "cycle": sat.cycle(),
        "epoch": sat.epoch().to_string(),
        "period": sat.period(),
        "block": sat.height().to_string(),
        "offset": sat.third(),
        "rarity": sat.rarity(),
        "satpoint": satpoint.unwrap_or(SatPoint {
          outpoint: OutPoint::null(),
            offset: 0,
          }),
        "blocktime": index.block_time(sat.height())?.timestamp().timestamp(),
        // "inscription": index.get_inscription_id_by_sat(sat)?
      }))
    )
  }

  pub fn _parse_inscriptions_from_witness(witnesses: Vec<Witness>) -> Vec<Value> {
    return ParsedEnvelope::from_witness(witnesses).into_iter().map(|envelope| {
      let inscription = envelope.payload;
      let mut content: String = "".to_owned();
      if let Some(_body) = inscription.clone().into_body() {
        content = general_purpose::STANDARD.encode(&_body);
      }
      json!({
        "content_length": inscription.content_length().unwrap_or(0),
        "content_type": inscription.content_type().unwrap_or(""),
        "content": content,
        "content_encoding": inscription.content_encoding().map_or(String::new(), |header_value| header_value.to_str().unwrap_or("").to_string()),
        "metaprotocol": inscription.metaprotocol(),
        "metadata":  match inscription.metadata() {
          Some(meta) => to_string(&meta).unwrap(),
          _ => "{}".to_owned(),
        },
        "parents": inscription.parents(),
        "delegate": inscription.delegate(),
        "unrecognized_even_field": inscription.unrecognized_even_field,
        "duplicate_field": inscription.duplicate_field,
        "incomplete_field": inscription.incomplete_field,
        "first_input": envelope.input == 0,
        "first_sat": envelope.offset == 0,
        "pointer": inscription.pointer.is_some(),
        "pushnum": envelope.pushnum,
        "stutter": envelope.stutter,
        "input": envelope.input,
        "offset": envelope.offset,
      })
    }).collect();
  }

  pub async fn parse_inscriptions_from_witness(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(witnesses): extract::Json<Vec<Vec<String>>>
  ) -> ServerResult<Json<Vec<Value>>> {
    let witnesses: Vec<Witness> = witnesses.iter().map(|witness| {
      let x: Vec<Vec<u8>> = witness.into_iter().map(|s|hex::decode(s).unwrap()).collect();
      return Witness::from(x);
    }).collect();
    Ok(
      Json(Self::_parse_inscriptions_from_witness(witnesses))
    )
  }

  pub async fn parse_inscriptions(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(txid): Path<Txid>,
  ) -> ServerResult<Json<Vec<Value>>> {
    let tx = index.get_raw_transaction(txid)?.unwrap();
    let witnesses = tx.input.iter().map(|input| input.clone().witness).collect();
    Ok(
      Json(Self::_parse_inscriptions_from_witness(witnesses))
    )
  }

  pub async fn parse_rune(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    Path(txid): Path<Txid>,
  ) -> ServerResult<Json<Option<Artifact>>> {
    let tx = index.get_raw_transaction(txid)?.unwrap();
    Ok(
      Json(Runestone::decipher(&tx))
    )
  }

  fn _output(
    outpoint: OutPoint,
    index: &Arc<Index>,
    chain: Chain
   ) -> Result<Value> {
    let sat_ranges = index.list(outpoint)?;

      let indexed;

      let output = if outpoint == OutPoint::null() || outpoint == unbound_outpoint() {
        let mut value = 0;

        if let Some(ranges) = &sat_ranges {
          for (start, end) in ranges {
            value += end - start;
          }
        }

        indexed = true;

        TxOut {
          value,
          script_pubkey: ScriptBuf::new(),
        }
      } else {
        indexed = index.contains_output(&outpoint)?;

        index
          .get_transaction(outpoint.txid).unwrap()
          .ok_or_not_found(|| format!("output {outpoint}")).unwrap()
          .output
          .into_iter()
          .nth(outpoint.vout as usize)
          .ok_or_not_found(|| format!("output {outpoint}")).unwrap()
      };

      let inscriptions = index.get_inscriptions_on_output(outpoint)?;

      let runes = index.get_rune_balances_for_outpoint(outpoint)?;

      let spent = index.is_output_spent(outpoint)?;
      let output = api::Output::new(
        chain,
        inscriptions,
        outpoint,
        output,
        indexed,
        runes,
        sat_ranges,
        spent,
      );
      Ok(json!({
        "address": output.address,
        "inscriptions": output.inscriptions,
        "outpoint": outpoint,
        "ranges": output.sat_ranges,
        "runes": output.runes
      }))
  }


  pub async fn outputs(
    Extension(server_config): Extension<Arc<ServerConfig>>,
    Extension(index): Extension<Arc<Index>>,
    extract::Json(outpoints): extract::Json<Vec<OutPoint>>,
  ) -> ServerResult<Json<Vec<Value>>> {
    let mut _outputs = vec![];

    for outpoint in outpoints {
      _outputs.push(Self::_output(outpoint, &index, server_config.chain)?);
    }
    Ok(Json(_outputs))
  }

}

fn _get_inscription(
  server_config: &Arc<ServerConfig>,
  index: &Arc<Index>,
  query: query::Inscription,
) -> Result<Value, ServerError>{
  let info = Index::inscription_info(&index, query)?.ok_or_not_found(|| format!("inscription {query}"))?;

  let raw_tx = index.get_raw_transaction(info.entry.id.txid)?;

  let current_inscription : Inscription = raw_tx.and_then(|tx| {
    ParsedEnvelope::from_transaction(&tx, &Some("".to_string()))
    .into_iter()
    .nth(info.entry.id.index as usize)
    .map(|envelope| envelope.payload)
  }).unwrap();

  let mut content: String = "".to_owned();
  if let Some(_body) = current_inscription.clone().into_body() {
    content = general_purpose::STANDARD.encode(&_body);
  }

  Ok(
    json!({
      "genesis_fee": info.entry.fee,
      "genesis_height": info.entry.height,
      "content_length": info.inscription.content_length(),
      "content_type": info.inscription.content_type().map(|s| s.to_string()),
      "content": content,
      "inscription_id": info.entry.id,
      "charms": Charm::charms(info.entry.charms),
      "address": info
      .output
      .as_ref()
      .and_then(|o| {
        server_config
          .chain
          .address_from_script(&o.script_pubkey)
          .ok()
      })
      .map(|address| address.to_string()),
      "number": info.entry.inscription_number,
      "output": json!({
        "value": info.output.as_ref().map(|o| o.value)
      }),
      "sat": info.entry.sat,
      "satpoint": info.satpoint,
      "metadata": match current_inscription.clone().metadata() {
        Some(meta) => match to_string(&meta) {
          Ok(v) => v,
          Err(_) => "{}".to_owned(),
        },
        _ => "{}".to_owned(),
      },
      "metaprotocol": current_inscription.metaprotocol(),
      "timestamp": info.entry.timestamp,
     })
  )
}
