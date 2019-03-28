use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use crossbeam::unbounded;
use env_logger;
use lazy_static::lazy_static;
use rosrust;
use std::process::Command;
use std::time;
use uuid::Uuid;

mod util;

mod msg {
    rosrust::rosmsg_include!(
        roscpp_tutorials / TwoInts,
        rospy_tutorials / AddTwoInts,
        std_msgs / String
    );
}

fn global_init() -> util::ChildProcessTerminator {
    env_logger::init();

    let roscore = util::run_roscore_for(util::Language::None, util::Feature::Benchmarks);
    rosrust::init("benchmarker");
    roscore
}

lazy_static! {
    static ref ROS_CORE: Option<util::ChildProcessTerminator> = Some(global_init());
}

fn setup() {
    assert!(ROS_CORE.is_some());
}

fn publish_subscribe(criterion: &mut Criterion) {
    setup();

    // let namespace = format!("/namespaceat{}", line!());

    let topic_name_inline = format!("topic_at_line_{}", line!());

    let (tx, rx) = unbounded();

    println!("Creating subscriber");

    let _subscriber_raii =
        rosrust::subscribe(&topic_name_inline, 200, move |v: msg::std_msgs::String| {
            println!("RECEIVING {}", v.data);
            tx.send(v.data).unwrap();
        })
        .unwrap();

    println!("Creating publisher");

    let publisher = rosrust::publish(&topic_name_inline, 200).unwrap();

    println!("Sending to subscriber until response is received");

    while rx.is_empty() {
        // println!("PUBLISHING");
        publisher
            .send(msg::std_msgs::String {
                data: "wait".into(),
            })
            .unwrap();
    }

    println!("Clearing response queue");

    while !rx.is_empty() {
        rx.recv().unwrap();
    }

    let publisher_inner = publisher.clone();
    criterion.bench_function("publish to inline subscriber once", move |b| {
        b.iter_batched(
            || Uuid::new_v4().to_simple().to_string(),
            |uuid| {
                publisher_inner
                    .send(msg::std_msgs::String { data: uuid.clone() })
                    .unwrap();
                assert_eq!(uuid, rx.recv().unwrap());
            },
            BatchSize::SmallInput,
        );
    });
}

fn call_service(criterion: &mut Criterion) {
    setup();

    let namespace = format!("/namespaceat{}", line!());

    let _roscpp_service = util::ChildProcessTerminator::spawn(
        Command::new("rosrun")
            .arg("roscpp_tutorials")
            .arg("add_two_ints_server")
            .arg(format!("__ns:={}", namespace))
            .arg("__name:=roscpp_service")
            .arg("add_two_ints:=add_two_ints_cpp"),
    );

    let _roscpp_service = util::ChildProcessTerminator::spawn(
        Command::new("rosrun")
            .arg("rospy_tutorials")
            .arg("add_two_ints_server")
            .arg(format!("__ns:={}", namespace))
            .arg("__name:=rospy_service")
            .arg("add_two_ints:=add_two_ints_py"),
    );

    let _roscpp_service = util::ChildProcessTerminator::spawn_example_bench(
        "../examples/serviceclient",
        Command::new("cargo")
            .arg("run")
            .arg("--release")
            .arg("--bin")
            .arg("service")
            .arg(format!("__ns:={}", namespace))
            .arg("__name:=rosrust_service")
            .arg("add_two_ints:=add_two_ints_rust"),
    );

    let service_name_inline = format!("service_at_line_{}", line!());

    let _service_raii =
        rosrust::service::<msg::roscpp_tutorials::TwoInts, _>(&service_name_inline, move |req| {
            let sum = req.a + req.b;
            Ok(msg::roscpp_tutorials::TwoIntsRes { sum })
        })
        .unwrap();

    let service_name_cpp = format!("{}/add_two_ints_cpp", namespace);
    let service_name_py = format!("{}/add_two_ints_py", namespace);
    let service_name_rust = format!("{}/add_two_ints_rust", namespace);

    rosrust::wait_for_service(&service_name_cpp, Some(time::Duration::from_secs(10))).unwrap();
    rosrust::wait_for_service(&service_name_py, Some(time::Duration::from_secs(10))).unwrap();
    rosrust::wait_for_service(&service_name_rust, Some(time::Duration::from_secs(10))).unwrap();
    rosrust::wait_for_service(&service_name_inline, Some(time::Duration::from_secs(10))).unwrap();

    let client_original =
        rosrust::client::<msg::roscpp_tutorials::TwoInts>(&service_name_cpp).unwrap();
    let client = client_original.clone();
    criterion.bench_function("call roscpp service once", move |b| {
        b.iter(|| {
            let sum = client
                .req(&msg::roscpp_tutorials::TwoIntsReq { a: 48, b: 12 })
                .unwrap()
                .unwrap()
                .sum;
            assert_eq!(60, sum);
        });
    });
    let client = client_original.clone();
    criterion.bench_function("call roscpp service 50 times in parallel", move |b| {
        b.iter(|| {
            let requests = (0..50)
                .map(|a| client.req_async(msg::roscpp_tutorials::TwoIntsReq { a, b: 5 }))
                .collect::<Vec<_>>();
            for (idx, request) in requests.into_iter().enumerate() {
                assert_eq!(idx as i64 + 5, request.read().unwrap().unwrap().sum);
            }
        });
    });

    let client_original =
        rosrust::client::<msg::rospy_tutorials::AddTwoInts>(&service_name_py).unwrap();
    let client = client_original.clone();
    criterion.bench_function("call rospy service once", move |b| {
        b.iter(|| {
            let sum = client
                .req(&msg::rospy_tutorials::AddTwoIntsReq { a: 48, b: 12 })
                .unwrap()
                .unwrap()
                .sum;
            assert_eq!(60, sum);
        });
    });

    let client_original =
        rosrust::client::<msg::roscpp_tutorials::TwoInts>(&service_name_rust).unwrap();
    let client = client_original.clone();
    criterion.bench_function("call rosrust service once", move |b| {
        b.iter(|| {
            let sum = client
                .req(&msg::roscpp_tutorials::TwoIntsReq { a: 48, b: 12 })
                .unwrap()
                .unwrap()
                .sum;
            assert_eq!(60, sum);
        });
    });
    let client = client_original.clone();
    criterion.bench_function("call rosrust service 50 times in parallel", move |b| {
        b.iter(|| {
            let requests = (0..50)
                .map(|a| client.req_async(msg::roscpp_tutorials::TwoIntsReq { a, b: 5 }))
                .collect::<Vec<_>>();
            for (idx, request) in requests.into_iter().enumerate() {
                assert_eq!(idx as i64 + 5, request.read().unwrap().unwrap().sum);
            }
        });
    });

    let client_original =
        rosrust::client::<msg::roscpp_tutorials::TwoInts>(&service_name_inline).unwrap();
    let client = client_original.clone();
    criterion.bench_function("call inline rosrust service once", move |b| {
        b.iter(|| {
            let sum = client
                .req(&msg::roscpp_tutorials::TwoIntsReq { a: 48, b: 12 })
                .unwrap()
                .unwrap()
                .sum;
            assert_eq!(60, sum);
        });
    });
    let client = client_original.clone();
    criterion.bench_function(
        "call inline rosrust service 50 times in parallel",
        move |b| {
            b.iter(|| {
                let requests = (0..50)
                    .map(|a| client.req_async(msg::roscpp_tutorials::TwoIntsReq { a, b: 5 }))
                    .collect::<Vec<_>>();
                for (idx, request) in requests.into_iter().enumerate() {
                    assert_eq!(idx as i64 + 5, request.read().unwrap().unwrap().sum);
                }
            });
        },
    );
}

criterion_group!(benches, publish_subscribe, call_service);
criterion_main!(benches);
