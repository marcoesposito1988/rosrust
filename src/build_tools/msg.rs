use regex::Regex;
use super::error::{Result, ResultExt};
use std::collections::{HashMap, HashSet};

pub struct Msg {
    pub package: String,
    pub name: String,
    pub fields: Vec<FieldInfo>,
    pub dependencies: HashSet<(String, String)>,
    pub source: String,
}

impl Msg {
    pub fn new(package: &str, name: &str, source: &str) -> Result<Msg> {
        let fields = match_lines(source)?;
        let dependencies = fields.iter()
            .filter_map(|ref v| match v.datatype {
                DataType::LocalStruct(ref dep_name) => {
                    Some((package.to_owned(), dep_name.to_owned()))
                }
                DataType::RemoteStruct(ref dep_pkg, ref dep_name) => {
                    Some((dep_pkg.to_owned(), dep_name.to_owned()))
                }
                _ => None,
            })
            .collect();
        Ok(Msg {
            package: package.to_owned(),
            name: name.to_owned(),
            fields: fields,
            dependencies: dependencies,
            source: source.into(),
        })
    }

    pub fn calculate_md5(&self,
                         hashes: &HashMap<(String, String), String>)
                         -> ::std::result::Result<String, ()> {
        use crypto::md5::Md5;
        use crypto::digest::Digest;
        let mut hasher = Md5::new();
        let constants = self.fields
            .iter()
            .filter(|ref v| v.is_constant())
            .map(|ref v| v.md5_string(&self.package, hashes))
            .collect::<::std::result::Result<Vec<String>, ()>>()?;
        let fields = self.fields
            .iter()
            .filter(|ref v| !v.is_constant())
            .map(|ref v| v.md5_string(&self.package, hashes))
            .collect::<::std::result::Result<Vec<String>, ()>>()?;
        let representation = constants.into_iter().chain(fields).collect::<Vec<_>>().join("\n");
        hasher.input_str(&representation);
        Ok(hasher.result_str())
    }
}

static IGNORE_WHITESPACE: &'static str = r"\s*";
static ANY_WHITESPACE: &'static str = r"\s+";
static FIELD_TYPE: &'static str = r"([a-zA-Z0-9_/]+)";
static FIELD_NAME: &'static str = r"([a-zA-Z][a-zA-Z0-9_]*)";
static EMPTY_BRACKETS: &'static str = r"\[\s*\]";
static NUMBER_BRACKETS: &'static str = r"\[\s*([0-9]+)\s*\]";

fn match_field(data: &str) -> Option<FieldLine> {
    lazy_static! {
        static ref MATCHER: String = format!("^{}{}{}$", FIELD_TYPE, ANY_WHITESPACE, FIELD_NAME);
        static ref RE: Regex = Regex::new(&MATCHER).unwrap();
    }
    let captures = match RE.captures(data) {
        Some(v) => v,
        None => return None,
    };
    Some(FieldLine {
        field_type: captures.get(1).unwrap().as_str().into(),
        field_name: captures.get(2).unwrap().as_str().into(),
    })
}

fn match_vector_field(data: &str) -> Option<FieldLine> {
    lazy_static! {
        static ref MATCHER: String = format!(
            "^{}{}{}{}{}$", FIELD_TYPE, IGNORE_WHITESPACE, EMPTY_BRACKETS, ANY_WHITESPACE,
            FIELD_NAME);
        static ref RE: Regex = Regex::new(&MATCHER).unwrap();
    }
    let captures = match RE.captures(data) {
        Some(v) => v,
        None => return None,
    };
    Some(FieldLine {
        field_type: captures.get(1).unwrap().as_str().into(),
        field_name: captures.get(2).unwrap().as_str().into(),
    })
}

fn match_array_field(data: &str) -> Option<(FieldLine, usize)> {
    lazy_static! {
        static ref MATCHER: String = format!(
            "^{}{}{}{}{}$", FIELD_TYPE, IGNORE_WHITESPACE, NUMBER_BRACKETS, ANY_WHITESPACE,
            FIELD_NAME);
        static ref RE: Regex = Regex::new(&MATCHER).unwrap();
    }
    let captures = match RE.captures(data) {
        Some(v) => v,
        None => return None,
    };
    Some((FieldLine {
              field_type: captures.get(1).unwrap().as_str().into(),
              field_name: captures.get(3).unwrap().as_str().into(),
          },
          captures.get(2).unwrap().as_str().parse().unwrap()))
}

fn match_const_string(data: &str) -> Option<(FieldLine, String)> {
    lazy_static! {
        static ref MATCHER: String = format!(
            r"^(string){}{}{}={}(.*)$", ANY_WHITESPACE, FIELD_NAME, IGNORE_WHITESPACE,
            IGNORE_WHITESPACE);
        static ref RE: Regex = Regex::new(&MATCHER).unwrap();
    }
    let captures = match RE.captures(data) {
        Some(v) => v,
        None => return None,
    };
    Some((FieldLine {
              field_type: captures.get(1).unwrap().as_str().into(),
              field_name: captures.get(2).unwrap().as_str().into(),
          },
          captures.get(3).unwrap().as_str().into()))
}

fn match_const_numeric(data: &str) -> Option<(FieldLine, String)> {
    lazy_static! {
        static ref MATCHER: String = format!(
            r"^{}{}{}{}={}(-?[0-9]+)$", FIELD_TYPE, ANY_WHITESPACE, FIELD_NAME,
            IGNORE_WHITESPACE, IGNORE_WHITESPACE);
        static ref RE: Regex = Regex::new(&MATCHER).unwrap();
    }
    let captures = match RE.captures(data) {
        Some(v) => v,
        None => return None,
    };
    Some((FieldLine {
              field_type: captures.get(1).unwrap().as_str().into(),
              field_name: captures.get(2).unwrap().as_str().into(),
          },
          captures.get(3).unwrap().as_str().into()))
}

fn match_line(data: &str) -> Option<Result<FieldInfo>> {
    if let Some((info, data)) = match_const_string(data.trim()) {
        return Some(FieldInfo::new(&info.field_type, &info.field_name, FieldCase::Const(data)));
    }
    let data = match strip_useless(data) {
        Ok(v) => v,
        Err(v) => return Some(Err(v)),
    };

    if data == "" {
        return None;
    }
    if let Some(info) = match_field(data) {
        return Some(FieldInfo::new(&info.field_type, &info.field_name, FieldCase::Unit));
    }
    if let Some(info) = match_vector_field(data) {
        return Some(FieldInfo::new(&info.field_type, &info.field_name, FieldCase::Vector));
    }
    if let Some((info, count)) = match_array_field(data) {
        return Some(FieldInfo::new(&info.field_type, &info.field_name, FieldCase::Array(count)));
    }
    if let Some((info, data)) = match_const_numeric(data) {
        return Some(FieldInfo::new(&info.field_type, &info.field_name, FieldCase::Const(data)));
    }
    Some(Err(format!("Unsupported content of line: {}", data).into()))
}

#[inline]
fn strip_useless<'a>(data: &'a str) -> Result<&'a str> {
    Ok(data.splitn(2, '#')
        .next()
        .ok_or_else(|| {
            format!("Somehow splitting a line resulted in 0 parts?! Happened here: {}",
                    data)
        })?
        .trim())
}

#[inline]
fn match_lines(data: &str) -> Result<Vec<FieldInfo>> {
    data.split('\n')
        .filter_map(match_line)
        .collect::<Result<_>>()
        .chain_err(|| "Failed to parse line in data string")
}

#[derive(Debug,PartialEq)]
struct FieldLine {
    field_type: String,
    field_name: String,
}

#[derive(Debug,PartialEq)]
pub enum FieldCase {
    Unit,
    Vector,
    Array(usize),
    Const(String),
}

#[derive(Debug,PartialEq)]
pub struct FieldInfo {
    datatype: DataType,
    name: String,
    case: FieldCase,
}

impl FieldInfo {
    fn is_constant(&self) -> bool {
        match self.case {
            FieldCase::Const(..) => true,
            _ => false,
        }
    }

    fn md5_string(&self,
                  package: &str,
                  hashes: &HashMap<(String, String), String>)
                  -> ::std::result::Result<String, ()> {
        let datatype = self.datatype.md5_string(package, hashes)?;
        Ok(match self.case {
            FieldCase::Unit => format!("{} {}", datatype, self.name),
            FieldCase::Vector => format!("{}[] {}", datatype, self.name),
            FieldCase::Array(l) => format!("{}[{}] {}", datatype, l, self.name),
            FieldCase::Const(ref v) => format!("{} {}={}", datatype, self.name, v),
        })
    }
}

impl FieldInfo {
    fn new(datatype: &str, name: &str, case: FieldCase) -> Result<FieldInfo> {
        Ok(FieldInfo {
            datatype: parse_datatype(&datatype).ok_or_else(
                || format!("Unsupported datatype: {}", datatype))?,
            name: name.to_owned(),
            case: case,
        })
    }
}

#[derive(Debug,PartialEq)]
pub enum DataType {
    Bool,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    String,
    Time,
    Duration,
    LocalStruct(String),
    RemoteStruct(String, String),
}

impl DataType {
    fn md5_string(&self,
                  package: &str,
                  hashes: &HashMap<(String, String), String>)
                  -> ::std::result::Result<String, ()> {
        Ok(match *self {
                DataType::Bool => "bool",
                DataType::I8 => "int8",
                DataType::I16 => "int16",
                DataType::I32 => "int32",
                DataType::I64 => "int64",
                DataType::U8 => "uint8",
                DataType::U16 => "uint16",
                DataType::U32 => "uint32",
                DataType::U64 => "uint64",
                DataType::F32 => "float32",
                DataType::F64 => "float64",
                DataType::String => "string",
                DataType::Time => "time",
                DataType::Duration => "duration",
                DataType::LocalStruct(ref name) => {
                    hashes.get(&(package.to_owned(), name.clone())).ok_or(())?.as_str()
                }
                DataType::RemoteStruct(ref pkg, ref name) => {
                    hashes.get(&(pkg.clone(), name.clone())).ok_or(())?.as_str()
                }
            }
            .into())
    }
}

fn parse_datatype(datatype: &str) -> Option<DataType> {
    match datatype {
        "bool" => Some(DataType::Bool),
        "int8" | "byte" => Some(DataType::I8),
        "int16" => Some(DataType::I16),
        "int32" => Some(DataType::I32),
        "int64" => Some(DataType::I64),
        "uint8" | "char" => Some(DataType::U8),
        "uint16" => Some(DataType::U16),
        "uint32" => Some(DataType::U32),
        "uint64" => Some(DataType::U64),
        "float32" => Some(DataType::F32),
        "float64" => Some(DataType::F64),
        "string" => Some(DataType::String),
        "time" => Some(DataType::Time),
        "duration" => Some(DataType::Duration),
        "Header" => Some(DataType::RemoteStruct("std_msgs".into(), "Header".into())),
        _ => {
            let parts = datatype.split('/').collect::<Vec<_>>();
            if parts.iter().any(|v| v.len() == 0) {
                return None;
            }
            match parts.len() {
                2 => Some(DataType::RemoteStruct(parts[0].to_owned(), parts[1].to_owned())),
                1 => Some(DataType::LocalStruct(parts[0].to_owned())),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datatype_md5_string_correct() {
        let mut hashes = HashMap::new();
        hashes.insert(("p1".into(), "xx".into()), "ABCD".into());
        hashes.insert(("p2".into(), "xx".into()), "EFGH".into());
        assert_eq!(DataType::I64.md5_string("", &hashes).unwrap(),
                   "int64".to_owned());
        assert_eq!(DataType::F32.md5_string("", &hashes).unwrap(),
                   "float32".to_owned());
        assert_eq!(DataType::String.md5_string("", &hashes).unwrap(),
                   "string".to_owned());
        assert_eq!(DataType::LocalStruct("xx".into()).md5_string("p1", &hashes).unwrap(),
                   "ABCD".to_owned());
        assert_eq!(DataType::LocalStruct("xx".into()).md5_string("p2", &hashes).unwrap(),
                   "EFGH".to_owned());
        assert_eq!(DataType::RemoteStruct("p1".into(), "xx".into())
                       .md5_string("p2", &hashes)
                       .unwrap(),
                   "ABCD".to_owned());
    }

    #[test]
    fn fieldinfo_md5_string_correct() {
        let mut hashes = HashMap::new();
        hashes.insert(("p1".into(), "xx".into()), "ABCD".into());
        hashes.insert(("p2".into(), "xx".into()), "EFGH".into());
        assert_eq!(FieldInfo::new("int64", "abc", FieldCase::Unit)
                       .unwrap()
                       .md5_string("", &hashes)
                       .unwrap(),
                   "int64 abc".to_owned());
        assert_eq!(FieldInfo::new("float32", "abc", FieldCase::Array(3))
                       .unwrap()
                       .md5_string("", &hashes)
                       .unwrap(),
                   "float32[3] abc".to_owned());
        assert_eq!(FieldInfo::new("string", "abc", FieldCase::Const("something".into()))
                       .unwrap()
                       .md5_string("", &hashes)
                       .unwrap(),
                   "string abc=something".to_owned());
        assert_eq!(FieldInfo::new("xx", "abc", FieldCase::Vector)
                       .unwrap()
                       .md5_string("p1", &hashes)
                       .unwrap(),
                   "ABCD[] abc".to_owned());
        assert_eq!(FieldInfo::new("p2/xx", "abc", FieldCase::Unit)
                       .unwrap()
                       .md5_string("p1", &hashes)
                       .unwrap(),
                   "EFGH abc".to_owned());
    }

    #[test]
    fn message_md5_string_correct() {
        assert_eq!(Msg::new("std_msgs", "String", "string data")
                       .unwrap()
                       .calculate_md5(&HashMap::new())
                       .unwrap(),
                   "992ce8a1687cec8c8bd883ec73ca41d1".to_owned());
        assert_eq!(Msg::new("geometry_msgs",
                            "Point",
                            include_str!("msg_examples/geometry_msgs/Point.msg"))
                       .unwrap()
                       .calculate_md5(&HashMap::new())
                       .unwrap(),
                   "4a842b65f413084dc2b10fb484ea7f17".to_owned());
        assert_eq!(Msg::new("geometry_msgs",
                            "Quaternion",
                            include_str!("msg_examples/geometry_msgs/Quaternion.msg"))
                       .unwrap()
                       .calculate_md5(&HashMap::new())
                       .unwrap(),
                   "a779879fadf0160734f906b8c19c7004".to_owned());
        let mut hashes = HashMap::new();
        hashes.insert(("geometry_msgs".into(), "Point".into()),
                      "4a842b65f413084dc2b10fb484ea7f17".into());
        hashes.insert(("geometry_msgs".into(), "Quaternion".into()),
                      "a779879fadf0160734f906b8c19c7004".into());
        assert_eq!(Msg::new("geometry_msgs",
                            "Pose",
                            include_str!("msg_examples/geometry_msgs/Pose.msg"))
                       .unwrap()
                       .calculate_md5(&hashes)
                       .unwrap(),
                   "e45d45a5a1ce597b249e23fb30fc871f".to_owned());
    }

    #[test]
    fn match_field_matches_legal_field() {
        assert_eq!(FieldLine {
                       field_type: "geom_msgs/Twist".into(),
                       field_name: "myname".into(),
                   },
                   match_field("geom_msgs/Twist   myname").unwrap());
    }

    #[test]
    fn match_vector_field_matches_legal_field() {
        assert_eq!(FieldLine {
                       field_type: "geom_msgs/Twist".into(),
                       field_name: "myname".into(),
                   },
                   match_vector_field("geom_msgs/Twist [  ]   myname").unwrap());
    }

    #[test]
    fn match_array_field_matches_legal_field() {
        assert_eq!((FieldLine {
                        field_type: "geom_msgs/Twist".into(),
                        field_name: "myname".into(),
                    },
                    127),
                   match_array_field("geom_msgs/Twist   [   127 ]   myname").unwrap());
    }

    #[test]
    fn match_const_string_matches_legal_field() {
        assert_eq!((FieldLine {
                        field_type: "string".into(),
                        field_name: "myname".into(),
                    },
                    "this is # data".into()),
                   match_const_string("string   myname  =  this is # data").unwrap());
    }

    #[test]
    fn match_const_numeric_matches_legal_field() {
        assert_eq!((FieldLine {
                        field_type: "mytype".into(),
                        field_name: "myname".into(),
                    },
                    "-444".into()),
                   match_const_numeric("mytype   myname  =  -444").unwrap());
    }

    #[test]
    fn match_line_works_on_legal_data() {
        assert!(match_line("#just a comment").is_none());
        assert!(match_line("#  YOLO !   ").is_none());
        assert!(match_line("      ").is_none());

        assert_eq!(FieldInfo {
                       datatype: DataType::RemoteStruct("geom_msgs".into(), "Twist".into()),
                       name: "myname".into(),
                       case: FieldCase::Unit,
                   },
                   match_line("  geom_msgs/Twist   myname    # this clearly should succeed")
                       .unwrap()
                       .unwrap());

        assert_eq!(FieldInfo {
                       datatype: DataType::RemoteStruct("geom_msgs".into(), "Twist".into()),
                       name: "myname".into(),
                       case: FieldCase::Vector,
                   },
                   match_line("  geom_msgs/Twist [  ]   myname  # ...").unwrap().unwrap());

        assert_eq!(FieldInfo {
                       datatype: DataType::U8,
                       name: "myname".into(),
                       case: FieldCase::Array(127),
                   },
                   match_line("  char   [   127 ]   myname# comment").unwrap().unwrap());
        assert_eq!(FieldInfo {
                       datatype: DataType::String,
                       name: "myname".into(),
                       case: FieldCase::Const("this is # data".into()),
                   },
                   match_line("  string  myname =   this is # data  ").unwrap().unwrap());
        assert_eq!(FieldInfo {
                       datatype: DataType::RemoteStruct("geom_msgs".into(), "Twist".into()),
                       name: "myname".into(),
                       case: FieldCase::Const("-444".into()),
                   },
                   match_line("  geom_msgs/Twist  myname =   -444 # data  ").unwrap().unwrap());
    }

    #[test]
    fn match_lines_parses_real_messages() {
        let data = match_lines(include_str!("msg_examples/geometry_msgs/TwistWithCovariance.msg"))
            .unwrap();
        assert_eq!(vec![FieldInfo {
                            datatype: DataType::LocalStruct("Twist".into()),
                            name: "twist".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::F64,
                            name: "covariance".into(),
                            case: FieldCase::Array(36),
                        }],
                   data);

        let data = match_lines(include_str!("msg_examples/geometry_msgs/PoseStamped.msg")).unwrap();
        assert_eq!(vec![FieldInfo {
                            datatype: DataType::RemoteStruct("std_msgs".into(), "Header".into()),
                            name: "header".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::LocalStruct("Pose".into()),
                            name: "pose".into(),
                            case: FieldCase::Unit,
                        }],
                   data);
    }

    #[test]
    fn msg_constructor_parses_real_message() {
        let data = Msg::new("geometry_msgs",
                            "TwistWithCovariance",
                            include_str!("msg_examples/geometry_msgs/TwistWithCovariance.msg"))
            .unwrap();
        assert_eq!(data.package, "geometry_msgs");
        assert_eq!(data.name, "TwistWithCovariance");
        assert_eq!(data.fields,
                   vec![FieldInfo {
                            datatype: DataType::LocalStruct("Twist".into()),
                            name: "twist".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::F64,
                            name: "covariance".into(),
                            case: FieldCase::Array(36),
                        }]);
        assert_eq!(data.dependencies.len(), 1);
        assert!(data.dependencies.contains(&("geometry_msgs".into(), "Twist".into())));

        let data = Msg::new("geometry_msgs",
                            "PoseStamped",
                            include_str!("msg_examples/geometry_msgs/PoseStamped.msg"))
            .unwrap();
        assert_eq!(data.package, "geometry_msgs");
        assert_eq!(data.name, "PoseStamped");
        assert_eq!(data.fields,
                   vec![FieldInfo {
                            datatype: DataType::RemoteStruct("std_msgs".into(), "Header".into()),
                            name: "header".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::LocalStruct("Pose".into()),
                            name: "pose".into(),
                            case: FieldCase::Unit,
                        }]);
        assert_eq!(data.dependencies.len(), 2);
        assert!(data.dependencies.contains(&("geometry_msgs".into(), "Pose".into())));
        assert!(data.dependencies.contains(&("std_msgs".into(), "Header".into())));

        let data = Msg::new("sensor_msgs",
                            "Imu",
                            include_str!("msg_examples/sensor_msgs/Imu.msg"))
            .unwrap();
        assert_eq!(data.package, "sensor_msgs");
        assert_eq!(data.name, "Imu");
        assert_eq!(data.fields,
                   vec![FieldInfo {
                            datatype: DataType::RemoteStruct("std_msgs".into(), "Header".into()),
                            name: "header".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::RemoteStruct("geometry_msgs".into(),
                                                             "Quaternion".into()),
                            name: "orientation".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::F64,
                            name: "orientation_covariance".into(),
                            case: FieldCase::Array(9),
                        },
                        FieldInfo {
                            datatype: DataType::RemoteStruct("geometry_msgs".into(),
                                                             "Vector3".into()),
                            name: "angular_velocity".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::F64,
                            name: "angular_velocity_covariance".into(),
                            case: FieldCase::Array(9),
                        },
                        FieldInfo {
                            datatype: DataType::RemoteStruct("geometry_msgs".into(),
                                                             "Vector3".into()),
                            name: "linear_acceleration".into(),
                            case: FieldCase::Unit,
                        },
                        FieldInfo {
                            datatype: DataType::F64,
                            name: "linear_acceleration_covariance".into(),
                            case: FieldCase::Array(9),
                        }]);
        assert_eq!(data.dependencies.len(), 3);
        assert!(data.dependencies.contains(&("geometry_msgs".into(), "Vector3".into())));
        assert!(data.dependencies.contains(&("geometry_msgs".into(), "Quaternion".into())));
        assert!(data.dependencies.contains(&("std_msgs".into(), "Header".into())));
    }
}
