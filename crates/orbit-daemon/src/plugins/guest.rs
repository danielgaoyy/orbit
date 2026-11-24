//! One fresh store per invocation. Host calls never re-enter the guest allocator.
use std::{
    io::ErrorKind,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use orbit_protocol::{
    OrbitPluginManifest, PLUGIN_MAX_FUEL, PLUGIN_MAX_MEMORY, valid_plugin_relative_path,
};
use wasmi::{
    Caller, Config, EnforcedLimits, Engine, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc, TypedResumableCall, WasmParams, WasmResults,
};

use super::io::{https_get, open_root, read_relative};

pub(super) const BUFFER_LIMIT: usize = 256 * 1024;
pub(super) const DEADLINE: Duration = Duration::from_secs(6);
const FUEL_QUANTUM: u64 = 250_000;

fn call_with_deadline<Params: WasmParams, Results: WasmResults>(
    func: &TypedFunc<Params, Results>,
    store: &mut Store<State>,
    params: Params,
    remaining_fuel: &mut u64,
    deadline: Instant,
) -> Result<Results> {
    let slice = (*remaining_fuel).min(FUEL_QUANTUM);
    ensure!(slice > 0, "plugin fuel budget exhausted");
    *remaining_fuel -= slice;
    store.set_fuel(slice)?;
    let mut call = func.call_resumable(&mut *store, params)?;
    loop {
        match call {
            TypedResumableCall::Finished(result) => {
                *remaining_fuel = remaining_fuel.saturating_add(store.get_fuel().unwrap_or(0));
                return Ok(result);
            }
            TypedResumableCall::OutOfFuel(handle) => {
                ensure!(Instant::now() < deadline, "plugin execution timed out");
                let next_slice = (*remaining_fuel).min(FUEL_QUANTUM);
                ensure!(next_slice > 0, "plugin fuel budget exhausted");
                *remaining_fuel -= next_slice;
                store.set_fuel(next_slice)?;
                call = handle.resume(&mut *store)?;
            }
            TypedResumableCall::HostTrap(trap) => {
                return Err(anyhow::Error::msg(trap.host_error().to_string()));
            }
        }
    }
}

pub(super) struct Guest {
    pub(super) manifest: OrbitPluginManifest,
    engine: Engine,
    module: Module,
    files: GlobSet,
}

struct State {
    guest: Arc<Guest>,
    root: Option<cap_std::fs::Dir>,
    limits: StoreLimits,
    deadline: Instant,
    calls_left: u32,
}

impl Guest {
    pub(super) fn load(manifest: OrbitPluginManifest, bytes: &[u8]) -> Result<Arc<Self>> {
        let mut files = GlobSetBuilder::new();
        for pattern in &manifest.capabilities.fs_read_globs {
            files.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
        }
        let mut config = Config::default();
        config
            .consume_fuel(true)
            .allow_start_fn(false)
            .wasm_multi_memory(false)
            .ignore_custom_sections(true)
            .enforced_limits(EnforcedLimits::strict());
        let engine = Engine::new(&config);
        let module = Module::new(&engine, bytes)?;
        let guest = Arc::new(Self {
            manifest,
            engine,
            module,
            files: files.build()?,
        });
        // Type-check imports, exports, initial tables and memory without running guest code.
        let (store, instance) = guest.instantiate(None, Instant::now() + DEADLINE)?;
        instance.get_typed_func::<i32, i32>(&store, "orbit_alloc")?;
        instance.get_typed_func::<(i32, i32), i64>(&store, "orbit_invoke")?;
        instance
            .get_memory(&store, "memory")
            .context("missing exported memory")?;
        if guest
            .module
            .imports()
            .any(|i| matches!(i.name(), "host_read_file" | "host_http_get"))
        {
            instance
                .get_global(&store, "orbit_host_buffer")
                .and_then(|g| g.get(&store).i32())
                .context("host I/O requires an exported i32 orbit_host_buffer")?;
        }
        Ok(guest)
    }

    fn instantiate(
        self: &Arc<Self>,
        root: Option<cap_std::fs::Dir>,
        deadline: Instant,
    ) -> Result<(Store<State>, wasmi::Instance)> {
        let caps = &self.manifest.capabilities;
        let limits = StoreLimitsBuilder::new()
            .memory_size(
                caps.max_memory_bytes
                    .unwrap_or(8 << 20)
                    .min(PLUGIN_MAX_MEMORY) as usize,
            )
            .table_elements(4096)
            .tables(1)
            .memories(1)
            .instances(1)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(
            &self.engine,
            State {
                guest: self.clone(),
                root,
                limits,
                deadline,
                calls_left: 64,
            },
        );
        store.limiter(|s| &mut s.limits);
        store.set_fuel(caps.max_fuel.unwrap_or(5_000_000).min(PLUGIN_MAX_FUEL))?;
        let mut linker = Linker::new(&self.engine);
        linker.func_wrap("orbit", "host_read_file", host_read_file)?;
        linker.func_wrap("orbit", "host_http_get", host_http_get)?;
        linker.func_wrap("orbit", "host_log", host_log)?;
        let instance = linker.instantiate_and_start(&mut store, &self.module)?;
        Ok((store, instance))
    }

    pub(super) fn run(
        self: &Arc<Self>,
        root: Option<&Path>,
        request: &[u8],
        deadline: Instant,
    ) -> Result<serde_json::Value> {
        ensure!(
            request.len() <= BUFFER_LIMIT && Instant::now() < deadline,
            "plugin request exceeds limits"
        );
        let (mut store, instance) = self.instantiate(root.map(open_root).transpose()?, deadline)?;
        let memory = instance
            .get_memory(&store, "memory")
            .context("missing exported memory")?;
        let alloc = instance.get_typed_func::<i32, i32>(&store, "orbit_alloc")?;
        let invoke = instance.get_typed_func::<(i32, i32), i64>(&store, "orbit_invoke")?;
        let mut remaining_fuel = self
            .manifest
            .capabilities
            .max_fuel
            .unwrap_or(5_000_000)
            .min(PLUGIN_MAX_FUEL);
        let len = i32::try_from(request.len())?;
        let ptr = call_with_deadline(&alloc, &mut store, len, &mut remaining_fuel, deadline)?;
        memory.write(&mut store, usize::try_from(ptr)?, request)?;
        let packed = call_with_deadline(
            &invoke,
            &mut store,
            (ptr, len),
            &mut remaining_fuel,
            deadline,
        )? as u64;
        let start = (packed >> 32) as usize;
        let size = packed as u32 as usize;
        ensure!(size <= BUFFER_LIMIT, "plugin result exceeds byte limit");
        let end = start
            .checked_add(size)
            .context("invalid plugin result range")?;
        let data = memory
            .data(&store)
            .get(start..end)
            .context("invalid plugin result pointer")?;
        Ok(serde_json::from_slice(data)?)
    }
}

type Trap = wasmi::Error;
fn trap(e: impl std::fmt::Display) -> Trap {
    Trap::new(e.to_string())
}

fn arg(c: &mut Caller<'_, State>, ptr: i32, len: i32) -> Result<(Memory, String), Trap> {
    let state = c.data_mut();
    if Instant::now() >= state.deadline || state.calls_left == 0 {
        return Err(trap("plugin host call budget exhausted"));
    }
    state.calls_left -= 1;
    let start = usize::try_from(ptr).map_err(trap)?;
    let len = usize::try_from(len).map_err(trap)?;
    if len > 4096 {
        return Err(trap("plugin host argument too long"));
    }
    let memory = c
        .get_export("memory")
        .and_then(|e| e.into_memory())
        .ok_or_else(|| trap("missing memory"))?;
    let mut data = vec![0; len];
    memory.read(&*c, start, &mut data).map_err(trap)?;
    Ok((memory, String::from_utf8(data).map_err(trap)?))
}

fn return_bytes(mut c: Caller<'_, State>, memory: Memory, bytes: &[u8]) -> Result<i64, Trap> {
    let ptr = c
        .get_export("orbit_host_buffer")
        .and_then(|e| e.into_global())
        .and_then(|g| g.get(&c).i32())
        .ok_or_else(|| trap("missing host buffer"))?;
    memory
        .write(&mut c, usize::try_from(ptr).map_err(trap)?, bytes)
        .map_err(trap)?;
    Ok((i64::from(ptr) << 32) | bytes.len() as i64)
}

fn host_read_file(mut c: Caller<'_, State>, ptr: i32, len: i32) -> Result<i64, Trap> {
    let (memory, path) = arg(&mut c, ptr, len)?;
    if !valid_plugin_relative_path(&path)
        || path.split('/').any(|seg| seg.eq_ignore_ascii_case(".git"))
        || !c.data().guest.files.is_match(&path)
    {
        return Err(trap("plugin file capability denied"));
    }
    let root = c
        .data()
        .root
        .as_ref()
        .ok_or_else(|| trap("plugin requires an authorized workspace"))?;
    match read_relative(root, &path, BUFFER_LIMIT) {
        Ok(bytes) => return_bytes(c, memory, &bytes),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == ErrorKind::NotFound) =>
        {
            Ok(-1)
        }
        Err(e) => Err(trap(e)),
    }
}

fn host_http_get(mut c: Caller<'_, State>, ptr: i32, len: i32) -> Result<i64, Trap> {
    let (memory, url) = arg(&mut c, ptr, len)?;
    let state = c.data();
    let remaining = state.deadline.saturating_duration_since(Instant::now());
    let bytes = tokio::runtime::Handle::current()
        .block_on(async {
            tokio::time::timeout(
                remaining,
                https_get(
                    &url,
                    &state.guest.manifest.capabilities.http_domains,
                    BUFFER_LIMIT,
                ),
            )
            .await
        })
        .map_err(trap)?
        .map_err(trap)?;
    return_bytes(c, memory, &bytes)
}

fn host_log(mut c: Caller<'_, State>, level: i32, ptr: i32, len: i32) -> Result<(), Trap> {
    let (_, msg) = arg(&mut c, ptr, len)?;
    tracing::info!(plugin_id=%c.data().guest.manifest.id, guest_level=level, message=?msg, "WASM plugin log");
    Ok(())
}
