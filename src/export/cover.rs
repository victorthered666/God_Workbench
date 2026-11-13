use crate::{AppError, Result};

pub fn calc_title_layout(title: &str, max_width_twips: usize, preferred: usize, minimum: usize) -> Result<(usize,Vec<String>)> {
    if title.trim().is_empty() || title.chars().count()>120 || max_width_twips==0 || max_width_twips>100_000 || minimum==0 || preferred<minimum || preferred>100 {
        return Err(AppError::Invalid("封面标题或页面宽度不合法".into()));
    }
    for size in (minimum..=preferred).rev().step_by(2) {
        let per_line=max_width_twips/(size*20);
        if per_line<2 { continue; }
        let lines=split_title(title,per_line)?;
        if lines.len()<=3 { return Ok((size,lines)); }
    }
    Ok((minimum,split_title(title,(max_width_twips/(minimum*20)).max(1))?))
}

pub fn split_title(title: &str, per_line: usize) -> Result<Vec<String>> {
    if title.is_empty() || per_line==0 { return Err(AppError::Invalid("标题和标题行宽不能为空".into())); }
    let per_line=per_line.clamp(2,120);
    let chars:Vec<char>=title.chars().collect();
    if chars.len()<=per_line { return Ok(vec![title.into()]); }
    let break_after="，。、；：！？的与和及之在于为-_—–·/ \t";
    let mut lines=vec![];
    let mut remaining=title.to_string();
    while remaining.chars().count()>per_line {
        let current:Vec<char>=remaining.chars().collect();
        let mut index=(per_line..=per_line).rev().find(|index|*index<current.len() && break_after.contains(current[*index-1]));
        if index.is_none() { index=(1..=per_line).rev().find(|index|*index<current.len() && break_after.contains(current[*index-1])); }
        let mut split=index.unwrap_or(per_line);
        if lines.is_empty() && split==per_line && current.len()>per_line { split=(split+1).min(current.len()); }
        if split==0 || split>current.len() { return Err(AppError::Invalid("标题换行未能推进".into())); }
        let fragment:String=current[..split].iter().collect::<String>().trim().into();
        let rest:String=current[split..].iter().collect::<String>().trim().into();
        if rest.chars().count()>=current.len() { return Err(AppError::Invalid("标题换行未能推进".into())); }
        if !fragment.is_empty() { lines.push(fragment); }
        remaining=rest;
    }
    if !remaining.is_empty() { lines.push(remaining); }
    if lines.len()>=2 && lines.last().unwrap().chars().count()<=2 {
        let last=lines.last().unwrap().chars().count();
        let previous=lines[lines.len()-2].chars().count();
        if previous+last<=per_line { let last=lines.pop().unwrap(); lines.last_mut().unwrap().push_str(&last); }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_title_breaking() {
        for width in [1,10,100,9906] {
            for title in ["a".repeat(120).as_str(),"星屿采集玩法设计（离线演示）"] {
                let (_,lines)=calc_title_layout(title,width,40,26).unwrap();
                assert!(!lines.is_empty());
                assert!(lines.iter().all(|line|!line.is_empty()));
            }
        }
        assert!(split_title("",0).is_err());
    }
}
