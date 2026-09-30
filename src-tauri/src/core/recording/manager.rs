pub struct RecordingManager {
    scopes: RwLock<HashMap<String, Arc<SessionRecorder>>>,
    session_scopes: RwLock<HashMap<String, String>>,
    memory_limit_bytes: AtomicUsize,
    app_handle: OnceLock<tauri::AppHandle>,
}

impl RecordingManager {
    pub fn new() -> Self {
        Self {
            scopes: RwLock::new(HashMap::new()),
            session_scopes: RwLock::new(HashMap::new()),
            memory_limit_bytes: AtomicUsize::new(DEFAULT_MEMORY_LIMIT_BYTES),
            app_handle: OnceLock::new(),
        }
    }

    pub fn set_app_handle(&self, app: tauri::AppHandle) {
        let _ = self.app_handle.set(app);
    }

    pub fn start_with_profile(
        &self,
        session_id: &str,
        context: RecordingContext,
        profile: RecordingProfile,
        explicit_path: Option<PathBuf>,
    ) -> AppResult<String> {
        let recorder = self.get_or_create_recorder(session_id);
        recorder.set_memory_limit(self.memory_limit_bytes.load(Ordering::Relaxed));

        if lock_recover(&recorder.runtime).is_recording() {
            return Err(AppError::Config("Recording is already active".to_string()));
        }
        {
            let mut transcript = lock_recover(&recorder.transcript);
            let _ = transcript.flush_output_lines(true);
        }
        let mut runtime = lock_recover(&recorder.runtime);
        if runtime.is_recording() {
            return Err(AppError::Config("Recording is already active".to_string()));
        }

        let path = match explicit_path {
            Some(path) => path,
            None => resolve_recording_path(&profile, &context, None)?,
        };

        let initial_bytes = match profile.mode {
            RecordingMode::Transcript if profile.include_session_metadata => {
                Some(format_session_header(&context).into_bytes())
            }
            _ => None,
        };
        let mode = profile.mode;
        let sink = RecordingSink::start(
            session_id,
            mode,
            path,
            profile.existing_file_behavior,
            self.app_handle.get(),
            initial_bytes,
        )?;
        let actual_path = sink.status.snapshot().file_path.clone();
        runtime.daily_key = local_day_key(context.started_at);
        runtime.context = Some(context);
        runtime.profile = Some(profile);
        runtime.sink = Some(sink);
        drop(runtime);

        self.record_system(
            session_id,
            format!(
                "Recording started ({})",
                match mode {
                    RecordingMode::Transcript => "transcript",
                    RecordingMode::Raw => "raw",
                }
            ),
        );
        Ok(actual_path)
    }

    pub fn stop(&self, session_id: &str) -> AppResult<String> {
        let recorder = self
            .get_recorder(session_id)
            .ok_or_else(|| AppError::Config("No active recording".to_string()))?;

        let flushed = {
            let mut transcript = lock_recover(&recorder.transcript);
            transcript.finish()
        };
        self.write_transcript_records(&recorder, &flushed);

        let (sink, footer) = {
            let mut runtime = lock_recover(&recorder.runtime);
            let Some(sink) = runtime.sink.take() else {
                return Err(AppError::Config("No active recording".to_string()));
            };
            let footer = match (runtime.profile.as_ref(), runtime.context.as_ref()) {
                (Some(profile), Some(context))
                    if profile.mode == RecordingMode::Transcript
                        && profile.include_session_metadata =>
                {
                    Some(format_session_footer(context, "Stopped").into_bytes())
                }
                _ => None,
            };
            runtime.profile = None;
            runtime.context = None;
            (sink, footer)
        };

        let status = sink.stop(footer);
        if let Some(error) = status.last_error {
            return Err(AppError::Config(format!(
                "Recording stopped with errors: {error}"
            )));
        }
        Ok(status.file_path)
    }

    pub fn save_transcript(
        &self,
        session_id: &str,
        file_path: &str,
        include_io_labels: bool,
        include_timestamps: bool,
    ) -> AppResult<String> {
        let path = prepare_output_file_path(file_path)?;
        let records = self
            .get_recorder(session_id)
            .map(|recorder| lock_recover(&recorder.transcript).snapshot_records())
            .unwrap_or_default();

        let mut writer = BufWriter::new(
            File::create(&path)
                .map_err(|e| AppError::Config(format!("Failed to create transcript file: {e}")))?,
        );
        for record in &records {
            writer
                .write_all(
                    record
                        .format(include_io_labels, include_timestamps)
                        .as_bytes(),
                )
                .map_err(|e| AppError::Config(format!("Failed to write transcript file: {e}")))?;
        }
        writer
            .flush()
            .map_err(|e| AppError::Config(format!("Failed to flush transcript file: {e}")))?;
        Ok(path.to_string_lossy().to_string())
    }

    pub fn search_history(
        &self,
        request: TerminalHistorySearchRequest,
    ) -> AppResult<TerminalHistorySearchResponse> {
        let started = Instant::now();
        let query = request.query;
        if query.is_empty() {
            return Ok(TerminalHistorySearchResponse {
                total: 0,
                elapsed_ms: started.elapsed().as_millis(),
                truncated: false,
                results: Vec::new(),
            });
        }

        let limit = request.limit.unwrap_or(DEFAULT_HISTORY_SEARCH_LIMIT).max(1);
        let context_before = request.context_before.unwrap_or(0).min(20);
        let context_after = request.context_after.unwrap_or(0).min(20);
        let max_lines = request
            .max_lines
            .unwrap_or(DEFAULT_HISTORY_SEARCH_LINES)
            .clamp(1, MAX_HISTORY_SEARCH_LINES);
        let records = self
            .get_recorder(&request.session_id)
            .map(|recorder| lock_recover(&recorder.transcript).snapshot_records())
            .unwrap_or_default();
        let start_index = records.len().saturating_sub(max_lines);
        let searched_records = &records[start_index..];
        let matcher = HistoryMatcher::new(
            &query,
            request.case_sensitive,
            request.regex,
            request.whole_word,
        )?;
        let mut total = 0usize;
        let mut results = Vec::new();

        for (relative_index, record) in searched_records.iter().enumerate() {
            if let Some((column_start, column_end)) = matcher.find(&record.data) {
                total += 1;
                if results.len() < limit {
                    let absolute_index = start_index + relative_index;
                    results.push(TerminalHistorySearchResult {
                        line_id: record.line_id,
                        line_number: absolute_index + 1,
                        column_start,
                        column_end,
                        preview: record.data.clone(),
                        before: context_records(&records, absolute_index, context_before, true),
                        after: context_records(&records, absolute_index, context_after, false),
                        source: record.kind.label().to_ascii_lowercase(),
                    });
                }
            }
        }

        Ok(TerminalHistorySearchResponse {
            total,
            elapsed_ms: started.elapsed().as_millis(),
            truncated: total > results.len() || records.len() > max_lines,
            results,
        })
    }

    pub fn set_memory_limit(&self, max_bytes: usize) {
        let bounded = max_bytes.max(1);
        self.memory_limit_bytes.store(bounded, Ordering::Relaxed);

        for state in rw_read_recover(&self.scopes).values() {
            state.set_memory_limit(bounded);
        }
    }

    pub fn is_recording(&self, session_id: &str) -> bool {
        self.get_recorder(session_id)
            .is_some_and(|recorder| lock_recover(&recorder.runtime).is_recording())
    }

    pub fn list_recording_sessions(&self) -> Vec<String> {
        rw_read_recover(&self.scopes)
            .values()
            .filter_map(|state| {
                lock_recover(&state.runtime)
                    .sink
                    .as_ref()
                    .map(|sink| sink.status.snapshot().session_id)
            })
            .collect()
    }

    pub fn get_recording_status(&self, session_id: &str) -> Option<RecordingStatus> {
        let recorder = self.get_recorder(session_id)?;
        let runtime = lock_recover(&recorder.runtime);
        runtime.sink.as_ref().map(|sink| sink.status.snapshot())
    }

    pub fn list_recording_statuses(&self) -> Vec<RecordingStatus> {
        rw_read_recover(&self.scopes)
            .values()
            .filter_map(|recorder| {
                lock_recover(&recorder.runtime)
                    .sink
                    .as_ref()
                    .map(|sink| sink.status.snapshot())
            })
            .collect()
    }

    pub fn write_output(&self, session_id: &str, data: &str) {
        let recorder = self.get_or_create_recorder(session_id);
        recorder.set_memory_limit(self.memory_limit_bytes.load(Ordering::Relaxed));
        let records = {
            let mut transcript = lock_recover(&recorder.transcript);
            transcript.write_output(data)
        };
        self.write_transcript_records(&recorder, &records);
    }

    pub fn write_raw_output(&self, session_id: &str, bytes: &[u8]) {
        let Some(recorder) = self.get_recorder(session_id) else {
            return;
        };
        let data = bytes.to_vec();
        let mut runtime = lock_recover(&recorder.runtime);
        if !should_write_raw(&runtime) {
            return;
        }
        maybe_rotate(&mut runtime);
        if let Some(sink) = runtime.sink.as_ref() {
            sink.enqueue(data);
        }
    }

    #[allow(dead_code)]
    pub fn write_input(&self, _session_id: &str, _data: &[u8]) {
        // Keystrokes are intentionally not recorded. Commands enter transcript
        // only through record_command_submission after submission/confirmation.
    }

    pub fn record_command_submission(
        &self,
        session_id: &str,
        command: String,
        sensitivity: InputSensitivity,
    ) {
        if sensitivity == InputSensitivity::Secret {
            return;
        }
        let recorder = self.get_or_create_recorder(session_id);
        let records = {
            let mut transcript = lock_recover(&recorder.transcript);
            transcript.record_command(command)
        };
        self.write_transcript_records(&recorder, &records);
    }

    pub fn record_system(&self, session_id: &str, message: impl Into<String>) {
        let recorder = self.get_or_create_recorder(session_id);
        let record = {
            let mut transcript = lock_recover(&recorder.transcript);
            transcript.record_system(message.into())
        };
        if let Some(record) = record {
            self.write_transcript_records(&recorder, &[record]);
        }
    }

    pub fn bind_session_scope(&self, session_id: &str, scope_id: &str) -> bool {
        let scope_id = if scope_id.is_empty() {
            session_id
        } else {
            scope_id
        };
        let (recorder, is_new) = {
            let mut scopes = rw_write_recover(&self.scopes);
            let is_new = !scopes.contains_key(scope_id);
            let recorder = scopes
                .entry(scope_id.to_string())
                .or_insert_with(|| {
                    Arc::new(SessionRecorder::new(
                        self.memory_limit_bytes.load(Ordering::Relaxed),
                    ))
                })
                .clone();
            (recorder, is_new)
        };
        rw_write_recover(&self.session_scopes).insert(session_id.to_string(), scope_id.to_string());
        {
            let mut lifecycle = lock_recover(&recorder.lifecycle);
            lifecycle.active_sessions.insert(session_id.to_string());
        }
        if !is_new {
            let runtime = lock_recover(&recorder.runtime);
            if let Some(sink) = runtime.sink.as_ref() {
                sink.status.set_session_id(session_id);
            }
            drop(runtime);
            self.record_system(session_id, "Session reconnected");
        }
        is_new
    }

    pub fn disconnect_session(&self, session_id: &str) {
        let Some(recorder) = self.get_recorder(session_id) else {
            return;
        };
        let flushed = lock_recover(&recorder.transcript).finish();
        self.write_transcript_records(&recorder, &flushed);
        self.record_system(session_id, "Session disconnected");
        let finalize = {
            let mut lifecycle = lock_recover(&recorder.lifecycle);
            lifecycle.active_sessions.remove(session_id);
            lifecycle.finalize_pending && lifecycle.active_sessions.is_empty()
        };
        if finalize {
            self.finish_scope(&self.resolve_scope(session_id));
        } else if self.resolve_scope(session_id) == session_id {
            self.finish_scope(session_id);
        }
    }

    pub fn finish_scope(&self, scope_id: &str) {
        let resolved = self.resolve_scope(scope_id);
        let scope_id = if rw_read_recover(&self.scopes).contains_key(scope_id) {
            scope_id
        } else {
            resolved.as_str()
        };
        let Some(recorder) = rw_read_recover(&self.scopes).get(scope_id).cloned() else {
            return;
        };
        {
            let mut lifecycle = lock_recover(&recorder.lifecycle);
            if !lifecycle.active_sessions.is_empty() {
                lifecycle.finalize_pending = true;
                return;
            }
        }
        let removed = rw_write_recover(&self.scopes).remove(scope_id);
        rw_write_recover(&self.session_scopes).retain(|_, scope| scope != scope_id);
        if let Some(recorder) = removed {
            let flushed = {
                let mut transcript = lock_recover(&recorder.transcript);
                transcript.finish()
            };
            self.write_transcript_records(&recorder, &flushed);
            let (sink, footer) = {
                let mut runtime = lock_recover(&recorder.runtime);
                let sink = runtime.sink.take();
                let footer = match (runtime.profile.as_ref(), runtime.context.as_ref()) {
                    (Some(profile), Some(context))
                        if profile.mode == RecordingMode::Transcript
                            && profile.include_session_metadata =>
                    {
                        Some(format_session_footer(context, "Session closed").into_bytes())
                    }
                    _ => None,
                };
                (sink, footer)
            };
            if let Some(sink) = sink {
                let _ = sink.stop(footer);
            }
        }
    }

    pub fn finish_all_scopes(&self) {
        let scope_ids: Vec<_> = rw_read_recover(&self.scopes).keys().cloned().collect();
        for scope_id in scope_ids {
            if let Some(recorder) = rw_read_recover(&self.scopes).get(&scope_id) {
                let mut lifecycle = lock_recover(&recorder.lifecycle);
                lifecycle.active_sessions.clear();
            }
            self.finish_scope(&scope_id);
        }
    }

    fn resolve_scope(&self, session_id: &str) -> String {
        rw_read_recover(&self.session_scopes)
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| session_id.to_string())
    }

    fn get_recorder(&self, session_id: &str) -> Option<Arc<SessionRecorder>> {
        rw_read_recover(&self.scopes)
            .get(&self.resolve_scope(session_id))
            .cloned()
    }

    fn get_or_create_recorder(&self, session_id: &str) -> Arc<SessionRecorder> {
        if let Some(recorder) = self.get_recorder(session_id) {
            return recorder;
        }
        let scope_id = self.resolve_scope(session_id);
        let mut scopes = rw_write_recover(&self.scopes);
        scopes
            .entry(scope_id)
            .or_insert_with(|| {
                Arc::new(SessionRecorder::new(
                    self.memory_limit_bytes.load(Ordering::Relaxed),
                ))
            })
            .clone()
    }

    fn write_transcript_records(
        &self,
        recorder: &Arc<SessionRecorder>,
        records: &[TranscriptRecord],
    ) {
        if records.is_empty() {
            return;
        }
        let mut runtime = lock_recover(&recorder.runtime);
        if !should_write_transcript(&runtime) {
            return;
        }
        maybe_rotate(&mut runtime);
        let Some(sink) = runtime.sink.as_ref() else {
            return;
        };
        let Some(profile) = runtime.profile.as_ref() else {
            return;
        };
        for record in records {
            sink.enqueue(
                record
                    .format(profile.include_io_labels, profile.include_timestamps)
                    .into_bytes(),
            );
        }
    }
}

fn should_write_transcript(runtime: &RecordingRuntime) -> bool {
    runtime
        .profile
        .as_ref()
        .is_some_and(|profile| profile.mode == RecordingMode::Transcript)
}

fn should_write_raw(runtime: &RecordingRuntime) -> bool {
    runtime.profile.as_ref().is_some_and(|profile| {
        let _include_binary_transfer_payloads = profile.include_binary_transfer_payloads;
        profile.mode == RecordingMode::Raw
    })
}

fn maybe_rotate(runtime: &mut RecordingRuntime) {
    let Some(profile) = runtime.profile.clone() else {
        return;
    };
    let Some(context) = runtime.context.clone() else {
        return;
    };
    let Some(sink) = runtime.sink.as_ref() else {
        return;
    };

    match profile.rotation {
        RotationPolicy::Session => {}
        RotationPolicy::Daily => {
            let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
            let next_key = local_day_key(now);
            if next_key != runtime.daily_key {
                let mut next_context = context.clone();
                next_context.started_at = now;
                if let Ok(path) =
                    resolve_recording_path(&profile, &next_context, None).and_then(|path| {
                        open_collision_safe_path(&path, profile.existing_file_behavior)
                    })
                {
                    sink.enqueue(
                        format_record_parts(
                            &chrono_timestamp(),
                            TranscriptEventKind::System.label(),
                            "Recording rotated",
                            profile.include_io_labels,
                            profile.include_timestamps,
                        )
                        .into_bytes(),
                    );
                    let header = (profile.mode == RecordingMode::Transcript
                        && profile.include_session_metadata)
                        .then(|| format_session_header(&next_context).into_bytes());
                    sink.rotate(path, profile.existing_file_behavior, header);
                    runtime.context = Some(next_context);
                    runtime.daily_key = next_key;
                }
            }
        }
        RotationPolicy::Size { max_bytes } => {
            if max_bytes > 0
                && sink
                    .status
                    .written_bytes
                    .load(Ordering::Relaxed)
                    .saturating_add(sink.status.queued_bytes.load(Ordering::Relaxed))
                    >= max_bytes
            {
                runtime.size_rotation_index = runtime.size_rotation_index.saturating_add(1);
                if let Ok(path) =
                    resolve_recording_path(&profile, &context, Some(runtime.size_rotation_index))
                        .and_then(|path| {
                            open_collision_safe_path(&path, profile.existing_file_behavior)
                        })
                {
                    sink.enqueue(
                        format_record_parts(
                            &chrono_timestamp(),
                            TranscriptEventKind::System.label(),
                            &format!("Recording rotated ({})", runtime.size_rotation_index),
                            profile.include_io_labels,
                            profile.include_timestamps,
                        )
                        .into_bytes(),
                    );
                    let header = (profile.mode == RecordingMode::Transcript
                        && profile.include_session_metadata)
                        .then(|| format_session_header(&context).into_bytes());
                    sink.rotate(path, profile.existing_file_behavior, header);
                    sink.status.written_bytes.store(0, Ordering::Relaxed);
                }
            }
        }
    }
}
