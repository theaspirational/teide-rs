//! Safe wrappers for Datalog engine FFI.

use std::ffi::CString;
use std::os::raw::c_int;
use crate::{Context, Error, Result, Table};
use crate::ffi;
use crate::ffi::datalog_ffi as dl;

/// Owns a dl_program_t. Keeps engine guard alive via embedded Context.
pub struct DlProgram {
    raw: *mut dl::dl_program_t,
    _ctx: Context,
}

impl DlProgram {
    pub fn new() -> Result<Self> {
        let ctx = Context::new()?;
        let raw = unsafe { dl::dl_program_new() };
        if raw.is_null() { return Err(Error::Oom); }
        Ok(DlProgram { raw, _ctx: ctx })
    }

    pub fn set_provenance_flag(&mut self) {
        unsafe { (*self.raw).flags |= dl::DL_FLAG_PROVENANCE; }
    }

    pub fn add_edb(&mut self, name: &str, table: &Table, arity: i32) -> Result<i32> {
        let c_name = CString::new(name).map_err(|_| Error::InvalidInput)?;
        let idx = unsafe { dl::dl_add_edb(self.raw, c_name.as_ptr(), table.as_raw(), arity as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_rule(&mut self, rule: &DlRule) -> Result<i32> {
        let idx = unsafe { dl::dl_add_rule(self.raw, &rule.raw) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn stratify(&mut self) -> Result<()> {
        let rc = unsafe { dl::dl_stratify(self.raw) };
        if rc != 0 { return Err(Error::InvalidInput); }
        Ok(())
    }

    pub fn eval(&mut self) -> Result<()> {
        let rc = unsafe { dl::dl_eval(self.raw) };
        if rc != 0 { return Err(Error::InvalidInput); }
        Ok(())
    }

    pub fn query(&self, pred_name: &str) -> Option<Table> {
        let c_name = CString::new(pred_name).ok()?;
        let ptr = unsafe { dl::dl_query(self.raw, c_name.as_ptr()) };
        if ptr.is_null() || ffi::td_is_err(ptr) { return None; }
        unsafe { ffi::td_retain(ptr) };
        unsafe { Table::from_raw(ptr).ok() }
    }

    pub fn mark_idb(&mut self, name: &str) {
        let c_name = match CString::new(name) { Ok(c) => c, Err(_) => return };
        let idx = unsafe { dl::dl_find_rel(self.raw, c_name.as_ptr()) };
        if idx >= 0 { unsafe { (*self.raw).rels[idx as usize].is_idb = true; } }
    }

    pub fn find_rel(&self, name: &str) -> Option<i32> {
        let c_name = CString::new(name).ok()?;
        let idx = unsafe { dl::dl_find_rel(self.raw, c_name.as_ptr()) };
        if idx < 0 { None } else { Some(idx) }
    }

    pub fn ensure_idb(&mut self, name: &str, arity: i32) -> Result<i32> {
        let c_name = CString::new(name).map_err(|_| Error::InvalidInput)?;
        let idx = unsafe { dl::dl_ensure_idb(self.raw, c_name.as_ptr(), arity as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub unsafe fn compile_rule(&mut self, rule_idx: usize, delta_pos: i32, g: *mut ffi::td_graph_t) -> Result<*mut ffi::td_op_t> {
        if rule_idx >= unsafe { (*self.raw).n_rules as usize } { return Err(Error::Range); }
        let rule_ptr = unsafe { &mut (*self.raw).rules[rule_idx] as *mut dl::dl_rule_t };
        let op = unsafe { dl::dl_compile_rule(self.raw, rule_ptr, delta_pos as c_int, rule_idx as c_int, g) };
        if op.is_null() { return Err(Error::InvalidInput); }
        Ok(op)
    }

    pub fn get_provenance_vec(&self, pred_name: &str) -> Option<Vec<i64>> {
        let c_name = CString::new(pred_name).ok()?;
        let ptr = unsafe { dl::dl_get_provenance(self.raw, c_name.as_ptr()) };
        if ptr.is_null() || ffi::td_is_err(ptr) { return None; }
        let len = unsafe { ffi::td_len(ptr) } as usize;
        if len == 0 { return Some(Vec::new()); }
        let data = unsafe { ffi::td_data(ptr) as *const i64 };
        let mut result = Vec::with_capacity(len);
        for i in 0..len { result.push(unsafe { *data.add(i) }); }
        Some(result)
    }

    pub fn get_provenance_sources(&self, pred_name: &str) -> Option<(Vec<i64>, Vec<i64>)> {
        let c_name = CString::new(pred_name).ok()?;
        let offsets_ptr = unsafe { dl::dl_get_provenance_src_offsets(self.raw, c_name.as_ptr()) };
        if offsets_ptr.is_null() || ffi::td_is_err(offsets_ptr) { return None; }
        let data_ptr = unsafe { dl::dl_get_provenance_src_data(self.raw, c_name.as_ptr()) };
        if data_ptr.is_null() || ffi::td_is_err(data_ptr) { return None; }
        let off_len = unsafe { ffi::td_len(offsets_ptr) } as usize;
        let data_len = unsafe { ffi::td_len(data_ptr) } as usize;
        let off_data = unsafe { ffi::td_data(offsets_ptr) as *const i64 };
        let data_data = unsafe { ffi::td_data(data_ptr) as *const i64 };
        let mut offsets = Vec::with_capacity(off_len);
        for i in 0..off_len { offsets.push(unsafe { *off_data.add(i) }); }
        let mut data = Vec::with_capacity(data_len);
        for i in 0..data_len { data.push(unsafe { *data_data.add(i) }); }
        Some((offsets, data))
    }

    pub fn as_raw(&self) -> *mut dl::dl_program_t { self.raw }
}

impl Drop for DlProgram {
    fn drop(&mut self) {
        if !self.raw.is_null() { unsafe { dl::dl_program_free(self.raw) }; }
    }
}

/// Stack-allocated rule with builder-pattern methods.
pub struct DlRule {
    pub(crate) raw: dl::dl_rule_t,
}

impl DlRule {
    pub fn new(head_pred: &str, head_arity: i32) -> Result<Self> {
        let c_pred = CString::new(head_pred).map_err(|_| Error::InvalidInput)?;
        let mut raw: dl::dl_rule_t = unsafe { std::mem::zeroed() };
        unsafe { dl::dl_rule_init(&mut raw, c_pred.as_ptr(), head_arity as c_int) };
        Ok(DlRule { raw })
    }

    pub fn head_var(&mut self, pos: i32, var_idx: i32) -> &mut Self {
        unsafe { dl::dl_rule_head_var(&mut self.raw, pos as c_int, var_idx as c_int) };
        self
    }

    pub fn head_const(&mut self, pos: i32, val: i64) -> &mut Self {
        unsafe { dl::dl_rule_head_const(&mut self.raw, pos as c_int, val) };
        self
    }

    pub fn add_atom(&mut self, pred: &str, arity: i32) -> Result<i32> {
        let c_pred = CString::new(pred).map_err(|_| Error::InvalidInput)?;
        let idx = unsafe { dl::dl_rule_add_atom(&mut self.raw, c_pred.as_ptr(), arity as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn body_set_var(&mut self, body_idx: i32, pos: i32, var_idx: i32) -> &mut Self {
        unsafe { dl::dl_body_set_var(&mut self.raw, body_idx as c_int, pos as c_int, var_idx as c_int) };
        self
    }

    pub fn body_set_const(&mut self, body_idx: i32, pos: i32, val: i64) -> &mut Self {
        unsafe { dl::dl_body_set_const(&mut self.raw, body_idx as c_int, pos as c_int, val) };
        self
    }

    pub fn add_neg(&mut self, pred: &str, arity: i32) -> Result<i32> {
        let c_pred = CString::new(pred).map_err(|_| Error::InvalidInput)?;
        let idx = unsafe { dl::dl_rule_add_neg(&mut self.raw, c_pred.as_ptr(), arity as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_cmp(&mut self, cmp_op: i32, lhs_var: i32, rhs_var: i32) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_cmp(&mut self.raw, cmp_op as c_int, lhs_var as c_int, rhs_var as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_cmp_const(&mut self, cmp_op: i32, lhs_var: i32, rhs_val: i64) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_cmp_const(&mut self.raw, cmp_op as c_int, lhs_var as c_int, rhs_val) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_assign(&mut self, target_var: i32, expr: &DlExpr) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_assign(&mut self.raw, target_var as c_int, dl::DL_OP_EQ as c_int, expr.raw) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_builtin(&mut self, builtin_id: i32, arity: i32) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_builtin(&mut self.raw, builtin_id as c_int, arity as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_cmp_expr(&mut self, cmp_op: i32, lhs: &DlExpr, rhs: &DlExpr) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_cmp_expr(&mut self.raw, cmp_op as c_int, lhs.raw, rhs.raw) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn add_interval(&mut self, fact_var: i32, start_var: i32, end_var: i32) -> Result<i32> {
        let idx = unsafe { dl::dl_rule_add_interval(&mut self.raw, fact_var as c_int, start_var as c_int, end_var as c_int) };
        if idx < 0 { return Err(Error::InvalidInput); }
        Ok(idx)
    }

    pub fn set_n_vars(&mut self, n: i32) -> &mut Self {
        self.raw.n_vars = n as c_int;
        self
    }
}

/// Expression tree node for assignment and comparison expressions.
pub struct DlExpr {
    raw: *mut dl::dl_expr_t,
}

impl DlExpr {
    pub fn constant(val: i64) -> Self {
        DlExpr { raw: unsafe { dl::dl_expr_const(val) } }
    }

    pub fn var(var_idx: i32) -> Self {
        DlExpr { raw: unsafe { dl::dl_expr_var(var_idx as c_int) } }
    }

    pub fn binop(op: i32, left: DlExpr, right: DlExpr) -> Self {
        DlExpr { raw: unsafe { dl::dl_expr_binop(op as c_int, left.raw, right.raw) } }
    }
}
