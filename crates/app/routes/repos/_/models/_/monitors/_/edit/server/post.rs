use crate::page::Page;
use anyhow::{bail, Result};
use pinwheel::prelude::*;
use std::{str, str::FromStr, sync::Arc};
use tangram_app_common::{
	alerts::{
		check_for_duplicate_monitor, delete_monitor, extract_threshold_bounds, get_monitor,
		update_monitor, validate_threshold_bounds, AlertCadence, AlertMethod, AlertMetric,
		AlertModelType, Monitor, MonitorThreshold, MonitorThresholdMode,
	},
	error::{bad_request, not_found, redirect_to_login, service_unavailable},
	model::get_model_bytes,
	path_components,
	user::{authorize_user, authorize_user_for_model, authorize_user_for_repo},
	Context,
};
use tangram_app_layouts::model_layout::{model_layout_info, ModelNavItem};
use tangram_id::Id;

#[derive(serde::Deserialize)]
#[serde(tag = "action")]
enum Action {
	#[serde(rename = "update_alert")]
	UpdateMonitor(UpdateMonitorAction),
	#[serde(rename = "delete")]
	Delete,
}

#[derive(serde::Deserialize)]
struct UpdateMonitorAction {
	cadence: String,
	email: String,
	metric: String,
	mode: String,
	threshold_lower: String,
	threshold_upper: String,
	title: String,
	webhook: String,
}

pub async fn post(request: &mut http::Request<hyper::Body>) -> Result<http::Response<hyper::Body>> {
	let context = request.extensions().get::<Arc<Context>>().unwrap().clone();
	let (repo_id, model_id, monitor_id) = if let ["repos", repo_id, "models", model_id, "monitors", monitor_id, "edit"] =
		*path_components(request).as_slice()
	{
		(
			repo_id.to_owned(),
			model_id.to_owned(),
			monitor_id.to_owned(),
		)
	} else {
		bail!("unexpected path");
	};
	let mut db = match context.database_pool.begin().await {
		Ok(db) => db,
		Err(_) => return Ok(service_unavailable()),
	};
	let user = match authorize_user(request, &mut db, context.options.auth_enabled()).await? {
		Ok(user) => user,
		Err(_) => return Ok(redirect_to_login()),
	};
	let repo_id: Id = match repo_id.parse() {
		Ok(repo_id) => repo_id,
		Err(_) => return Ok(not_found()),
	};
	if !authorize_user_for_repo(&mut db, &user, repo_id).await? {
		return Ok(not_found());
	}
	let model_id: Id = match model_id.parse() {
		Ok(model_id) => model_id,
		Err(_) => return Ok(bad_request()),
	};
	if !authorize_user_for_model(&mut db, &user, model_id).await? {
		return Ok(not_found());
	}
	let data = match hyper::body::to_bytes(request.body_mut()).await {
		Ok(data) => data,
		Err(_) => return Ok(bad_request()),
	};
	let action: Action = match serde_urlencoded::from_bytes(&data) {
		Ok(action) => action,
		Err(_) => {
			dbg!(data);
			return Ok(bad_request());
		}
	};
	let bytes = get_model_bytes(&context.storage, model_id).await?;
	let model = tangram_model::from_bytes(&bytes)?;
	let model_type = AlertModelType::from(model.inner());
	let model_layout_info =
		model_layout_info(&mut db, &context, model_id, ModelNavItem::Monitors).await?;
	match action {
		Action::Delete => {
			delete_monitor(&mut db, &monitor_id).await?;
			db.commit().await?;
			let response = http::Response::builder()
				.status(http::StatusCode::SEE_OTHER)
				.header(
					http::header::LOCATION,
					format!("/repos/{}/models/{}/monitors/", repo_id, model_id),
				)
				.body(hyper::Body::empty())
				.unwrap();
			Ok(response)
		}
		Action::UpdateMonitor(um) => {
			let UpdateMonitorAction {
				cadence,
				email,
				metric,
				mode,
				threshold_lower,
				threshold_upper,
				title,
				webhook,
			} = um;
			let metric = AlertMetric::from_str(&metric)?;
			// Validate metric type
			let mut methods = vec![AlertMethod::Stdout];
			if !email.is_empty() {
				methods.push(AlertMethod::Email(email));
			}
			if !webhook.is_empty() {
				methods.push(AlertMethod::Webhook(webhook));
			}
			let threshold_bounds = validate_threshold_bounds(threshold_lower, threshold_upper);
			if threshold_bounds.is_none() {
				let page = Page {
					monitor: get_monitor(&mut db, &monitor_id).await?,
					monitor_id,
					model_layout_info,
					model_type,
					error: Some("Must provide at least one threshold bound.".to_owned()),
				};
				let html = html(page);
				let response = http::Response::builder()
					.status(http::StatusCode::BAD_REQUEST)
					.body(hyper::Body::from(html))
					.unwrap();
				return Ok(response);
			}
			let (variance_lower, variance_upper) =
				extract_threshold_bounds(threshold_bounds.unwrap())?;
			let mut monitor = Monitor {
				cadence: AlertCadence::from_str(&cadence)?,
				id: Id::generate(),
				methods,
				model_id,
				threshold: MonitorThreshold {
					metric,
					mode: MonitorThresholdMode::from_str(&mode)?,
					variance_lower,
					variance_upper,
				},
				title,
			};
			if monitor.title.is_empty() {
				monitor.title = monitor.default_title();
			}
			if check_for_duplicate_monitor(&mut db, &monitor, model_id).await? {
				let page = Page {
					monitor,
					monitor_id,
					model_layout_info,
					model_type,
					error: Some("Identical monitor already exists.".to_owned()),
				};
				let html = html(page);
				let response = http::Response::builder()
					.status(http::StatusCode::BAD_REQUEST)
					.body(hyper::Body::from(html))
					.unwrap();
				return Ok(response);
			}
			let result = update_monitor(&mut db, &monitor, &monitor_id).await;
			if result.is_err() {
				let page = Page {
					monitor,
					monitor_id,
					model_layout_info,
					model_type,
					error: Some("There was an error editing your monitor.".to_owned()),
				};
				let html = html(page);
				let response = http::Response::builder()
					.status(http::StatusCode::BAD_REQUEST)
					.body(hyper::Body::from(html))
					.unwrap();
				return Ok(response);
			};
			db.commit().await?;
			let response = http::Response::builder()
				.status(http::StatusCode::SEE_OTHER)
				.header(
					http::header::LOCATION,
					format!("/repos/{}/models/{}/monitors/", repo_id, model_id),
				)
				.body(hyper::Body::empty())
				.unwrap();
			Ok(response)
		}
	}
}